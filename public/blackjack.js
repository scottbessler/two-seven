import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings } from "/public/card-settings.js";
import { cents, money, responseError } from "/public/shared.js";
// Shared renderer contract: card-pip-${position}.

const root = document.getElementById("blackjack-app");

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

  useEffect(() => {
    fetch("/blackjack/resume")
      .then((response) => response.ok ? response.json() : null)
      .then((value) => value && setGame(value))
      .catch(() => {});
  }, []);

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
    <${CardSettings} />
    <form id="blackjack-form" onSubmit=${start}>
      <label>Bet ($)<input name="bet" type="number" min="1" max="10000" step="0.01" value="25.00" /></label>
    </form>
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
      ` : html`<button form="blackjack-form" disabled=${busy}>Deal</button>`}
    </div>
    ${error && html`<p class="error">${error}</p>`}
  </section>`;
}

if (root) render(html`<${App} />`, root);
