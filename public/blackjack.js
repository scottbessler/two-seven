import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings } from "/public/card-settings.js";
import { money, responseError, wholeDollarMoney } from "/public/shared.js";
// Shared renderer contract: card-corner rank over suit.

const root = document.getElementById("blackjack-app");

// Mirrors bet_options in src/blackjack.rs: a nibble, a real bet, and a big one,
// capped at half the bankroll so a double or split remains affordable.
function betOptions(balance) {
  if (balance < 100) return [];
  const maxStart = Math.min(balance, Math.max(100, Math.floor(balance / 2 / 100) * 100));
  const rounded = [Math.min(balance / 100, 10000), balance / 20, balance / 4]
    .map((bet) => Math.min(maxStart, Math.max(100, Math.floor(bet / 100) * 100)));
  return [...new Set([...rounded, maxStart])].toSorted((left, right) => left - right);
}

function Hand({ title, cards, score, hidden }) {
  return html`<section class="blackjack-hand">
    <h2>${title}${score == null ? "" : ` · ${score}`}</h2>
    <div class="board">
      ${cards.map((card) => html`<${Card} value=${card} interactive=${true} />`)}
      ${hidden && html`<${Card} hidden=${true} interactive=${true} />`}
    </div>
  </section>`;
}

function App() {
  const [game, setGame] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [balance, setBalance] = useState(0);

  const loadBalance = () => fetch("/api/bank", { headers: { Accept: "application/json" } })
    .then((response) => (response.ok ? response.json() : null))
    .then((account) => account && setBalance(account.balance))
    .catch(() => {});

  useEffect(() => {
    loadBalance();
    fetch("/blackjack/resume")
      .then((response) => response.ok ? response.json() : null)
      .then((value) => value && setGame(value))
      .catch(() => {});
  }, []);

  const start = async (amount) => {
    setBusy(true);
    setError("");
    const response = await fetch("/blackjack/start", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ bet: amount }),
    });
    setBusy(false);
    if (!response.ok) {
      setError(await responseError(response));
      return;
    }
    setGame(await response.json());
    loadBalance();
  };

  const act = async (kind) => {
    setBusy(true);
    setError("");
    const response = await fetch(`/blackjack/${kind}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: game.id }),
    });
    setBusy(false);
    if (!response.ok) {
      setError(await responseError(response));
      return;
    }
    setGame(await response.json());
    loadBalance();
  };

  const bets = betOptions(balance);
  return html`<section class="blitz-table blackjack-table">
    <${CardSettings} interactive=${true} />
    ${game && html`<div class="blitz-score">
      <span><b>${money(game.bet)}</b> bet</span>
      <span><b>${game.payout ? money(game.payout) : "—"}</b> payout</span>
      <span><b>${game.status}</b> status</span>
    </div>
    <${Hand} title="Dealer" cards=${game.dealer} score=${game.dealer_score} hidden=${game.dealer_score == null} />
    ${game.hands.map((hand, index) => html`<${Hand} title=${`Hand ${index + 1}${index === game.active_hand ? " · Active" : ""}`} cards=${hand.cards} score=${hand.score} />`)}
    <p class="blitz-feedback">${game.message}</p>
    `}
    <div class="actions blackjack-actions">
      ${game?.status === "Playing" ? html`
        ${game.can_hit && html`<button type="button" disabled=${busy} onClick=${() => act("hit")}>Hit</button>`}
        ${game.can_stand && html`<button type="button" disabled=${busy} onClick=${() => act("stand")}>Stand</button>`}
        ${game.can_double && html`<button type="button" disabled=${busy} onClick=${() => act("double")}>Double</button>`}
        ${game.can_split && html`<button type="button" disabled=${busy} onClick=${() => act("split")}>Split</button>`}
        ${game.can_insure && html`<button type="button" disabled=${busy} onClick=${() => act("insurance")}>Insurance</button>`}
      ` : bets.length === 0
        ? html`<span class="deal-broke">Re-up from the coin menu to play a hand.</span>`
        : bets.map((amount) => html`<button class="deal-action" type="button" disabled=${busy} onClick=${() => start(amount)}>Deal ${wholeDollarMoney(amount)}</button>`)}
    </div>
    ${error && html`<p class="error">${error}</p>`}
  </section>`;
}

if (root) render(html`<${App} />`, root);
