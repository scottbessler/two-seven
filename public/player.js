import { announceBank, money, responseError, wholeDollarMoney as chips } from "/public/shared.js";

// Money moves in whole $1,000 chips, so the amount is a stepper rather than a
// field: there is no wrong number to type, and the buttons go inert at the
// ends of what you can afford.
const panel = document.querySelector(".gift-panel");
const controls = panel?.querySelector(".gift-controls");

// The gift lands in both ledgers and redraws the chart, so the page is re-read
// rather than patched. The gift panel itself stays put: it holds the focused
// control and the message that says what just happened.
async function refreshPlayerPanels() {
  const response = await fetch(window.location.href, { headers: { Accept: "text/html" }, cache: "no-store" });
  if (!response.ok) return;
  const page = new DOMParser().parseFromString(await response.text(), "text/html");
  for (const selector of [".player-summary", ".loans-panel", ".gifts-panel", ".chart-panel", ".ledger-panel"]) {
    const current = document.querySelector(selector);
    const fresh = page.querySelector(selector);
    if (!fresh) continue;
    // The first gift is also the first time the gifts panel exists, so it is
    // placed where the server puts it rather than swapped in.
    if (current) current.replaceWith(fresh);
    else document.querySelector(".chart-panel")?.before(fresh);
  }
}

// One press clears every loan the balance covers. The panel is server-rendered
// against your debt, so the page is re-read afterwards rather than patched --
// the button, the summary and the ledger all move together.
document.addEventListener("click", async (event) => {
  const button = event.target.closest(".loans-repay-all");
  if (!button || button.disabled) return;
  const status = button.parentElement?.querySelector(".loans-status");
  button.disabled = true;
  button.ariaBusy = "true";
  try {
    const response = await fetch("/api/bank/repay-all", {
      method: "POST",
      headers: { Accept: "application/json", "Content-Type": "application/json" },
      body: "{}",
    });
    if (response.ok) {
      announceBank(await response.json());
      await refreshPlayerPanels();
      return;
    }
    if (status) status.textContent = await responseError(response);
  } catch {
    if (status) status.textContent = "That did not go through. Try again.";
  }
  button.disabled = false;
  button.ariaBusy = "false";
});

if (panel && controls) {
  const increment = Number(panel.dataset.increment) || 100_000;
  const amountLabel = panel.querySelector(".gift-amount");
  const balanceLabel = panel.querySelector(".gift-balance");
  const send = panel.querySelector(".gift-send");
  const status = panel.querySelector(".gift-status");
  let balance = Number(panel.dataset.balance) || 0;
  let amount = increment;
  let inFlight = false;

  const ceiling = () => Math.max(increment, Math.floor(balance / increment) * increment);
  const draw = () => {
    amount = Math.min(Math.max(amount, increment), ceiling());
    amountLabel.textContent = chips(amount);
    for (const step of panel.querySelectorAll(".gift-step")) {
      const next = amount + Number(step.dataset.step) * increment;
      step.disabled = inFlight || next < increment || next > ceiling();
    }
    send.disabled = inFlight || balance < amount;
    send.textContent = `Send ${chips(amount)}`;
    send.ariaBusy = String(inFlight);
  };

  for (const step of panel.querySelectorAll(".gift-step")) {
    step.addEventListener("click", () => {
      amount += Number(step.dataset.step) * increment;
      draw();
    });
  }

  send.addEventListener("click", async () => {
    if (inFlight) return;
    inFlight = true;
    status.textContent = "";
    draw();
    const sent = amount;
    try {
      const response = await fetch(`/player/${panel.dataset.playerId}/gift`, {
        method: "POST",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify({ amount: sent }),
      });
      if (response.ok) {
        const result = await response.json();
        balance = result.account.balance;
        panel.dataset.balance = String(balance);
        if (balanceLabel) balanceLabel.textContent = money(balance);
        announceBank(result.account);
        status.textContent = `Sent ${chips(sent)} to ${result.recipient.name}.`;
        await refreshPlayerPanels();
      } else status.textContent = await responseError(response);
    } catch {
      status.textContent = "That did not go through. Try again.";
    } finally {
      inFlight = false;
      draw();
    }
  });

  draw();
}

// Account options are the server's business -- it is what enforces them -- so
// each toggle posts the whole set and reports what came back.
const options = document.querySelector(".options-panel");
if (options) {
  const status = options.querySelector(".option-status");
  const boxes = {
    unfunded_tournaments: options.querySelector("[name=unfunded-tournaments]"),
    see_bot_cards: options.querySelector("[name=see-bot-cards]"),
  };
  const save = async () => {
    const body = Object.fromEntries(Object.entries(boxes).map(([key, box]) => [key, box.checked]));
    for (const box of Object.values(boxes)) box.disabled = true;
    try {
      const response = await fetch("/player/settings", {
        method: "POST",
        headers: { Accept: "application/json", "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (response.ok) {
        const saved = await response.json();
        for (const [key, box] of Object.entries(boxes)) box.checked = Boolean(saved[key]);
        status.textContent = "Saved.";
      } else status.textContent = await responseError(response);
    } catch {
      status.textContent = "That did not save. Try again.";
    } finally {
      for (const box of Object.values(boxes)) box.disabled = false;
    }
  };
  for (const box of Object.values(boxes)) box.addEventListener("change", save);
}
