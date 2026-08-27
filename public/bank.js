import { announceBank, refreshBank, wholeDollarMoney as money } from "/public/shared.js";

document.documentElement.classList.toggle(
  "standalone-pwa",
  window.matchMedia("(display-mode: standalone)").matches || navigator.standalone === true,
);

// iOS reports env(safe-area-inset-bottom) as 0 for a beat after a standalone
// PWA cold-launches, so the action bar's bottom padding briefly collapses to
// nothing. Re-measure the real inset with a probe element and republish it as
// --safe-bottom once WebKit settles, instead of trusting env() at first paint.
function syncSafeAreaInsetBottom() {
  const probe = document.createElement("div");
  probe.style.cssText =
    "position:fixed;left:0;bottom:0;width:0;height:env(safe-area-inset-bottom);visibility:hidden;pointer-events:none";
  document.body.appendChild(probe);
  const measured = probe.getBoundingClientRect().height;
  probe.remove();
  if (measured > 0) document.documentElement.style.setProperty("--safe-bottom", `${measured}px`);
}

if (document.documentElement.classList.contains("standalone-pwa")) {
  syncSafeAreaInsetBottom();
  for (const delay of [50, 300, 1000]) setTimeout(syncSafeAreaInsetBottom, delay);
  window.visualViewport?.addEventListener("resize", syncSafeAreaInsetBottom);
  window.addEventListener("orientationchange", () => setTimeout(syncSafeAreaInsetBottom, 80));
  window.addEventListener("pageshow", syncSafeAreaInsetBottom);
}

if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js").catch(() => {});

// The header total comes from the bank, but the rest of a lobby page is
// rendered by the server against the balance you had when it loaded: which
// tables you can afford, and which are filed under "out of reach". A re-up
// leaves that markup a step behind, and so does money won or lost anywhere
// else. Re-render those lists from a fresh copy of the page rather than making
// the player reload it themselves.
async function refreshLobbyLists() {
  const current = [...document.querySelectorAll(".table-list")];
  if (current.length === 0) return;
  const response = await fetch(window.location.href, { headers: { Accept: "text/html" }, cache: "no-store" });
  if (!response.ok) return;
  const page = new DOMParser().parseFromString(await response.text(), "text/html");
  const fresh = [...page.querySelectorAll(".table-list")];
  if (fresh.length !== current.length) return;
  current.forEach((section, index) => {
    // An open disclosure stays open across the swap; it was a deliberate click.
    const opened = section.querySelector("details.out-of-reach")?.open;
    const replacement = fresh[index];
    const details = replacement.querySelector("details.out-of-reach");
    if (details && opened) details.open = true;
    section.replaceWith(replacement);
  });
}

// Nothing re-runs when a page comes back from the back/forward cache, so it
// still shows the balance it was drawn with. Ask the bank again -- on the way
// back in, and whenever the tab is brought forward after a spell away.
window.addEventListener("pageshow", (event) => {
  if (!event.persisted) return;
  refreshBank().catch(() => {});
  refreshLobbyLists().catch(() => {});
});
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState !== "visible") return;
  refreshBank().catch(() => {});
  refreshLobbyLists().catch(() => {});
});

const balance = document.getElementById("bank-balance");
const delta = document.getElementById("bank-delta");
const widget = document.querySelector(".bank-widget");
const panel = document.getElementById("bank-panel");

function netChangeInLastHour(entries) {
  const cutoff = Date.now() - 60 * 60 * 1000;
  return entries
    .filter((entry) => {
      const at = Date.parse(entry.at);
      return Number.isFinite(at) && at >= cutoff;
    })
    .reduce((sum, entry) => sum + entry.delta, 0);
}

if (balance && widget && panel) {
  const summary = widget.querySelector("summary");
  const heading = document.createElement("strong");
  const loanBadge = document.createElement("span");
  loanBadge.className = "loan-badge";
  const debt = document.createElement("span");
  debt.className = "loan-summary";
  const reUp = document.createElement("button");
  reUp.type = "button";
  reUp.className = "bank-action re-up-button";
  reUp.textContent = "Re-up $1,000";
  const repay = document.createElement("button");
  repay.type = "button";
  repay.className = "bank-action repay-button";
  const playerLink = document.createElement("a");
  playerLink.className = "player-page-link";
  playerLink.href = "/player";
  playerLink.textContent = "Player page";
  const entries = document.createElement("div");
  entries.className = "bank-entries";
  panel.replaceChildren(heading, loanBadge, debt, reUp, repay, playerLink, entries);
  let currentAccount = null;

  const bankMutation = async (endpoint, button) => {
    button.disabled = true;
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { Accept: "application/json", "Content-Type": "application/json" },
      body: "{}",
    });
    if (!response.ok) {
      button.disabled = false;
      return;
    }
    widget.open = false;
    announceBank(await response.json());
    await refreshLobbyLists();
  };
  reUp.addEventListener("click", (event) => {
    event.stopPropagation();
    bankMutation("/api/bank", reUp).catch(() => {
      reUp.disabled = currentAccount?.can_re_up !== true;
    });
  });
  repay.addEventListener("click", (event) => {
    event.stopPropagation();
    bankMutation("/api/bank/repay", repay).catch(() => {
      repay.disabled = (currentAccount?.balance ?? 0) < (currentAccount?.next_repayment_amount ?? Infinity);
    });
  });

  document.addEventListener("click", (event) => {
    if (widget.open && !widget.contains(event.target)) widget.open = false;
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && widget.open) {
      widget.open = false;
      summary.focus();
    }
  });
  const showBank = (account) => {
    currentAccount = account;
    balance.textContent = money(account.balance);
    const recentNet = netChangeInLastHour(account.entries || []);
    delta.textContent = recentNet ? ` (${recentNet >= 0 ? "+" : ""}${money(recentNet)})` : "";
    const canReUp = account.can_re_up === true;
    for (const button of document.querySelectorAll(".re-up-form button")) {
      button.disabled = !canReUp;
    }
    heading.textContent = `Balance ${money(account.balance)}`;
    loanBadge.textContent = `Loans ${account.loan_count ?? 0}`;
    debt.textContent = `Debt ${money(account.loan_debt ?? 0)} · Net ${money(account.net_balance ?? account.balance)}`;
    reUp.disabled = !canReUp;
    const nextRepayment = account.next_repayment_amount;
    if (nextRepayment != null) {
      repay.textContent = `Pay back ${money(nextRepayment)}`;
      repay.disabled = account.balance < nextRepayment;
      repay.hidden = false;
    } else {
      repay.hidden = true;
    }
    entries.replaceChildren();
    for (const entry of account.entries.slice(-5).toReversed()) {
      const line = document.createElement("div");
      line.textContent = `${entry.delta >= 0 ? "+" : ""}${money(entry.delta)} ${entry.memo}`;
      entries.append(line);
    }
  };
  window.addEventListener("bank:updated", (event) => {
    if (event.detail) showBank(event.detail);
  });
  refreshBank().catch(() => {});
  for (const form of document.querySelectorAll(".re-up-form")) {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      fetch("/api/bank", {
        method: "POST",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: "{}",
      })
        .then((response) => (response.ok ? response.json() : Promise.reject(response)))
        .then((account) => {
          announceBank(account);
          return refreshLobbyLists();
        })
        .catch(() => {});
    });
  }
}
