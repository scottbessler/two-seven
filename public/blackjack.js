import { html, render, useState } from "/public/vendor/htm-preact.js";

const root = document.getElementById("blackjack-app");

function cents(value) {
  return Math.round(Number(value) * 100);
}

function money(value) {
  const sign = value < 0 ? "-" : "";
  const abs = Math.abs(value);
  return `${sign}$${Math.floor(abs / 100).toLocaleString()}.${String(abs % 100).padStart(2, "0")}`;
}

function cardFace(value) {
  const suitCode = value.slice(-1);
  const rawRank = value.slice(0, -1);
  const rank = rawRank === "T" ? "10" : rawRank;
  const suit = { h: "♥", d: "♦", c: "♣", s: "♠" }[suitCode] || suitCode;
  const numeric = { A: 1, K: 13, Q: 12, J: 11, T: 10 }[rawRank] || Number(rawRank);
  return { suitCode, rank, suit, numeric };
}

const PIP_POSITIONS = {
  2: ["top-center", "bottom-center"],
  3: ["top-center", "middle-center", "bottom-center"],
  4: ["top-left", "top-right", "bottom-left", "bottom-right"],
  5: ["top-left", "top-right", "middle-center", "bottom-left", "bottom-right"],
  6: ["top-left", "top-right", "middle-left", "middle-right", "bottom-left", "bottom-right"],
  7: ["top-left", "top-right", "middle-left", "middle-right", "bottom-left", "bottom-right", "upper-center"],
  8: ["top-left", "top-right", "middle-left", "middle-right", "bottom-left", "bottom-right", "upper-center", "lower-center"],
  9: ["top-left", "top-right", "upper-left", "upper-right", "middle-center", "lower-left", "lower-right", "bottom-left", "bottom-right"],
  10: ["top-left", "top-right", "upper-left", "upper-right", "middle-left", "middle-right", "lower-left", "lower-right", "bottom-left", "bottom-right"],
};

function Card({ value, hidden = false }) {
  if (hidden) return html`<span class="playing-card empty-card" aria-label="Hidden card">?</span>`;
  const { suitCode, rank, suit, numeric } = cardFace(value);
  const court = { 1: "A", 11: "J", 12: "Q", 13: "K" }[numeric];
  return html`<span class=${suitCode === "h" || suitCode === "d" ? "playing-card red" : "playing-card black"} aria-label=${value}>
    <span class="card-corner"><b>${rank}</b><i>${suit}</i></span>
    ${court
      ? html`<span class="card-art card-art-${court}"><i>${suit}</i><b>${court}</b></span>`
      : html`<span class="pip-grid pip-grid-${numeric}">${PIP_POSITIONS[numeric].map((position) => html`<i class=${`card-pip-${position}`}>${suit}</i>`)}</span>`}
    <span class="card-corner card-corner-bottom"><b>${rank}</b><i>${suit}</i></span>
  </span>`;
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
      setError("Unable to deal blackjack");
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
      setError("That blackjack game is no longer available");
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
    <${Hand} title="Player" cards=${game.player} score=${game.player_score} />
    <p class="blitz-feedback">${game.message}</p>
    <div class="actions blackjack-actions">
      <button type="button" disabled=${busy || !game.can_hit} onClick=${() => act("hit")}>Hit</button>
      <button type="button" disabled=${busy || !game.can_stand} onClick=${() => act("stand")}>Stand</button>
    </div>`}
    ${error && html`<p class="error">${error}</p>`}
  </section>`;
}

if (root) render(html`<${App} />`, root);
