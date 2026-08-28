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
  for (const selector of [".player-summary", ".chart-panel", ".ledger-panel"]) {
    const current = document.querySelector(selector);
    const fresh = page.querySelector(selector);
    if (current && fresh) current.replaceWith(fresh);
  }
}

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
