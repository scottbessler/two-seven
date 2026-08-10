import { html, render, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
// Shared renderer contract: card-pip-${position}.

const root = document.getElementById("blackjack-app");

function cents(value) {
  return Math.round(Number(value) * 100);
}

function money(value) {
  const sign = value < 0 ? "-" : "";
  const abs = Math.abs(value);
  return `${sign}$${Math.floor(abs / 100).toLocaleString()}.${String(abs % 100).padStart(2, "0")}`;
}

async function responseError(response) {
  const text = await response.text();
  try {
    return JSON.parse(text).error || text;
  } catch {
    return text || `Request failed (${response.status})`;
  }
}

function Hand({ title, cards, score, hidden }) {
  return html`<section class="blackjack-hand">
    <h2>${title}${score == null ? "" : ` · ${score}`}</h2>
    <div class="board">
      ${cards.map((card) => html`<${Card} value=${card} />`)}
      ${hidden && html`<${Card} hidden=${true} />`}
    </div>
  </section>`;
}

function App() {
  const [game, setGame] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const start = async (event) => {
    event.preventDefault();
    const bet = cents(new FormData(event.currentTarget).get("bet"));
    setBusy(true);
    setError("");
    const response = await fetch("/blackjack/start", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ bet }),
    });
    setBusy(false);
    if (!response.ok) {
      setError(await responseError(response));
      return;
    }
    setGame(await response.json());
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
  };

  return html`<section class="blitz-table blackjack-table">
    <form id="blackjack-form" onSubmit=${start}>
      <label>Bet ($)<input name="bet" type="number" min="0.01" step="0.01" value="25.00" /></label>
      <button disabled=${busy}>Deal</button>
    </form>
    ${game && html`<div class="blitz-score">
      <span><b>${money(game.bet)}</b> bet</span>
      <span><b>${game.payout ? money(game.payout) : "—"}</b> payout</span>
      <span><b>${game.status}</b> status</span>
    </div>
    <${Hand} title="Dealer" cards=${game.dealer} score=${game.dealer_score} hidden=${game.dealer_score == null} />
    ${game.hands.map((hand, index) => html`<${Hand} title=${`Hand ${index + 1}${index === game.active_hand ? " · Active" : ""}`} cards=${hand.cards} score=${hand.score} />`)}
    <p class="blitz-feedback">${game.message}</p>
    <div class="actions blackjack-actions">
      <button type="button" disabled=${busy || !game.can_hit} onClick=${() => act("hit")}>Hit</button>
      <button type="button" disabled=${busy || !game.can_stand} onClick=${() => act("stand")}>Stand</button>
      <button type="button" disabled=${busy || !game.can_double} onClick=${() => act("double")}>Double</button>
      <button type="button" disabled=${busy || !game.can_split} onClick=${() => act("split")}>Split</button>
      <button type="button" disabled=${busy || !game.can_insure} onClick=${() => act("insurance")}>Insurance</button>
    </div>`}
    ${error && html`<p class="error">${error}</p>`}
  </section>`;
}

if (root) render(html`<${App} />`, root);
