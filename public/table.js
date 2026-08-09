import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";

const root = document.getElementById("table-app");
const tableId = root?.dataset.tableId;

async function fetchState() {
  const response = await fetch(`/tables/${tableId}/state`, { headers: { Accept: "application/json" } });
  if (!response.ok) throw new Error("Unable to load table state");
  return response.json();
}

function actionName(action) { return typeof action === "string" ? action : Object.keys(action)[0]; }

function Card({ card }) {
  return html`<span class="playing-card ${card?.suit === "h" || card?.suit === "d" ? "red" : "black"}">${card || "?"}</span>`;
}

function Seat({ seat, button }) {
  return html`<article class="seat ${seat.index === button ? "dealer" : ""}">
    <strong>${seat.occupant}</strong><span>Seat ${seat.index}</span><b>$${(seat.stack / 100).toFixed(2)}</b>
    ${seat.index === button && html`<i class="button-marker">D</i>`}
  </article>`;
}

function Actions({ hand, tableId, refresh }) {
  const [amount, setAmount] = useState(0);
  const actions = (hand?.legal_actions?.actions || []).map(actionName);
  const submit = async (kind) => {
    await fetch(`/tables/${tableId}/action`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ kind, amount: Number(amount) || undefined }) });
    refresh();
  };
  return html`<div class="actions" aria-label="Actions">
    ${actions.some((a) => a === "Fold") && html`<button class="danger" onClick=${() => submit("fold")}>Fold</button>`}
    ${actions.some((a) => a === "Check") && html`<button onClick=${() => submit("check")}>Check</button>`}
    ${actions.some((a) => a === "Call") && html`<button onClick=${() => submit("call")}>Call ${hand.to_call ? `$${(hand.to_call / 100).toFixed(2)}` : ""}</button>`}
    ${(actions.some((a) => a === "Bet") || actions.some((a) => a === "Raise")) && html`<input type="number" min="${hand.legal_actions.wager?.min || 0}" max="${hand.legal_actions.wager?.max || 0}" value=${amount} onInput=${(event) => setAmount(event.target.value)} aria-label="Wager amount" />`}
    ${actions.some((a) => a === "Bet") && html`<button onClick=${() => submit("bet")}>Bet</button>`}
    ${actions.some((a) => a === "Raise") && html`<button onClick=${() => submit("raise")}>Raise</button>`}
    ${actions.some((a) => a === "AllIn") && html`<button onClick=${() => submit("all_in")}>All in</button>`}
  </div>`;
}

function TableApp() {
  const [state, setState] = useState(null);
  const refresh = () => fetchState().then(setState).catch(() => {});
  useEffect(() => {
    refresh();
    const events = new EventSource(`/tables/${tableId}/events`);
    events.addEventListener("state", (event) => setState(JSON.parse(event.data)));
    events.addEventListener("error", refresh);
    return () => events.close();
  }, []);
  if (!state) return html`<p class="loading">Loading table…</p>`;
  const hand = state.hand;
  return html`<div class="table-shell">
    <div class="table-top"><h1>${state.name}</h1><span class="pot">Pot ${hand ? `$${(hand.pot / 100).toFixed(2)}` : "—"}</span></div>
    <section class="felt" aria-label="Poker table">
      <div class="board">${(hand?.board || []).map((card) => html`<${Card} card=${card} />`)}</div>
      <div class="seats">${state.seats.map((seat) => html`<${Seat} seat=${seat} button=${state.button} />`)}</div>
    </section>
    ${hand && html`<section class="hand-info"><p>Street: ${hand.street} · Turn: seat ${hand.current_player ?? "—"}</p><div class="hole-cards">${(hand.your_hole_cards || []).map((card) => html`<${Card} card=${card} />`)}</div><${Actions} hand=${hand} tableId=${tableId} refresh=${refresh} /></section>`}
    ${state.last_hand && html`<section class="card"><h2>Last hand</h2><p>${state.last_hand.awards.map((award) => `Seat ${award.seat} won $${(award.amount / 100).toFixed(2)}`).join(" · ")}</p></section>`}
    <section class="card join-card"><h2>Join this table</h2><form onSubmit=${async (event) => { event.preventDefault(); const data = new FormData(event.currentTarget); await fetch(`/tables/${tableId}/join`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ seat: Number(data.get("seat")), buy_in: Number(data.get("buy_in")) }) }); refresh(); }}><input name="seat" type="number" min="0" max="${state.seats.length - 1}" placeholder="Seat" required /><input name="buy_in" type="number" min="1" placeholder="Buy-in cents" required /><button>Join</button></form></section>
    <nav class="table-controls"><button onClick=${() => fetch(`/tables/${tableId}/sit`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ sitting_out: true }) }).then(refresh)}>Sit out</button><button onClick=${() => fetch(`/tables/${tableId}/leave`, { method: "POST" }).then(refresh)}>Leave</button></nav>
  </div>`;
}

if (root) render(html`<${TableApp} />`, root);
