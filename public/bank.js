import { announceBank, refreshBank, wholeDollarMoney as money } from "/public/shared.js";

if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js").catch(() => {});

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
  const showBank = (account) => {
    balance.textContent = money(account.balance);
    const recentNet = netChangeInLastHour(account.entries || []);
    delta.textContent = recentNet ? ` (${recentNet >= 0 ? "+" : ""}${money(recentNet)})` : "";
    for (const button of document.querySelectorAll(".re-up-form button")) {
      button.disabled = account.balance >= 10_000;
    }
    panel.replaceChildren();
    const heading = document.createElement("strong");
    heading.textContent = `Balance ${money(account.balance)}`;
    panel.append(heading);
    const shame = document.createElement("span");
    shame.className = "loan-badge";
    shame.textContent = `Loans ${account.loan_count ?? 0}`;
    panel.append(shame);
    const debt = document.createElement("span");
    debt.className = "loan-summary";
    debt.textContent = `Debt ${money(account.loan_debt ?? 0)} · Net ${money(account.net_balance ?? account.balance)}`;
    panel.append(debt);
    const reUp = document.createElement("button");
    reUp.type = "button";
    reUp.className = "re-up-button";
    reUp.textContent = "Re-up $1,000";
    reUp.disabled = account.balance >= 10_000;
    reUp.addEventListener("click", (event) => {
      event.stopPropagation();
      reUp.disabled = true;
      fetch("/api/bank", {
        method: "POST",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: "{}",
      })
        .then((response) => (response.ok ? response.json() : Promise.reject(response)))
        .then((updated) => announceBank(updated))
        .catch(() => {
          reUp.disabled = account.balance >= 10_000;
        });
    });
    panel.append(reUp);
    const nextRepayment = account.next_repayment_amount;
    if (nextRepayment != null) {
      const repay = document.createElement("button");
      repay.type = "button";
      repay.className = "re-up-button repay-button";
      repay.textContent = `Pay back ${money(nextRepayment)}`;
      repay.disabled = account.balance < nextRepayment;
      repay.addEventListener("click", (event) => {
        event.stopPropagation();
        repay.disabled = true;
        fetch("/api/bank/repay", {
          method: "POST",
          headers: { Accept: "application/json", "Content-Type": "application/json" },
          body: "{}",
        })
          .then((response) => (response.ok ? response.json() : Promise.reject(response)))
          .then((updated) => announceBank(updated))
          .catch(() => {
            repay.disabled = account.balance < nextRepayment;
          });
      });
      panel.append(repay);
    }
    const playerLink = document.createElement("a");
    playerLink.className = "player-page-link";
    playerLink.href = "/player";
    playerLink.textContent = "Player page";
    panel.append(playerLink);
    for (const entry of account.entries.slice(-5).toReversed()) {
      const line = document.createElement("div");
      line.textContent = `${entry.delta >= 0 ? "+" : ""}${money(entry.delta)} ${entry.memo}`;
      panel.append(line);
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
        .then((account) => announceBank(account))
        .catch(() => {});
    });
  }
}
