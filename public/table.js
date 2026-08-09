import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";

const root = document.getElementById("table-app");
const tableId = root?.dataset.tableId;

async function fetchState() {
  const response = await fetch(`/tables/${tableId}/state`, { headers: { Accept: "application/json" } });
  if (!response.ok) throw new Error("Unable to load table state");
  return response.json();
}

function actionName(action) {
  return typeof action === "string" ? action : Object.keys(action)[0];
}

function cents(value) {
  return `$${(value / 100).toFixed(2)}`;
}

function Card({ card, empty = false }) {
  const suit = card?.slice(-1);
  return html`<span class="playing-card ${suit === "h" || suit === "d" ? "red" : "black"} ${empty ? "empty-card" : ""}">${card || "·"}</span>`;
}

function Seat({ seat, button, openSeat, total }) {
  const angle = -Math.PI / 2 + (seat.index * 2 * Math.PI) / total;
  const position = {
    left: `${50 + 43 * Math.cos(angle)}%`,
    top: `${50 + 40 * Math.sin(angle)}%`,
    transform: "translate(-50%, -50%)",
  };
  if (seat.occupant === "empty") {
    return html`<button class="seat empty-seat" style=${position} onClick=${() => openSeat(seat.index)}><strong>🪙 Sit here</strong><span>Seat ${seat.index}</span></button>`;
  }
  const label = seat.display_name || seat.occupant;
  const shared = seat.occupant !== "human" && seat.occupant !== "empty";
  return html`<article class="seat ${seat.index === button ? "dealer" : ""}" style=${position}>
    <strong>${label} <span class="coin">🪙</span></strong>
    <span>Seat ${seat.index}${shared ? " · bot bank" : ""}</span>
    <b>${cents(seat.stack)}</b>
    ${seat.bank_balance != null && html`<small class="seat-bank" title=${seat.bank_entries.map((entry) => entry.memo).join(", ")}>${cents(seat.bank_balance)}</small>`}
    ${seat.index === button && html`<i class="button-marker">D</i>`}
  </article>`;
}

function Actions({ hand, tableId: actionTableId, refresh }) {
  const wager = hand?.legal_actions?.wager;
  const actions = new Set((hand?.legal_actions?.actions || []).map(actionName));
  const minimum = wager?.min || wager?.fixed || 0;
  const [amount, setAmount] = useState(minimum);
  useEffect(() => setAmount(minimum), [minimum]);
  const submit = async (kind) => {
    await fetch(`/tables/${actionTableId}/action`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ kind, amount: Number(amount) || undefined }) });
    refresh();
  };
  const noLimit = wager?.max != null && wager?.min != null;
  return html`<div class="actions" aria-label="Actions">
    ${actions.has("Fold") && html`<button class="danger" onClick=${() => submit("fold")}>Fold</button>`}
    ${actions.has("Check") && html`<button onClick=${() => submit("check")}>Check</button>`}
    ${actions.has("Call") && html`<button onClick=${() => submit("call")}>Call ${hand.to_call ? cents(hand.to_call) : ""}</button>`}
    ${(actions.has("Bet") || actions.has("Raise")) && html`
      <div class="wager-control">
        ${noLimit && html`<input type="range" min=${wager.min} max=${wager.max} value=${amount} onInput=${(event) => setAmount(event.target.value)} aria-label="Wager slider" />`}
        ${noLimit && html`<input type="number" min=${wager.min} max=${wager.max} value=${amount} onInput=${(event) => setAmount(event.target.value)} aria-label="Wager amount" />`}
        ${noLimit && html`<div class="pot-shortcuts"><button type="button" onClick=${() => setAmount(Math.min(wager.max, hand.pot / 2))}>½ pot</button><button type="button" onClick=${() => setAmount(Math.min(wager.max, hand.pot))}>Pot</button></div>`}
        ${!noLimit && html`<span class="fixed-wager">Fixed ${cents(minimum)}</span>`}
      </div>
    `}
    ${actions.has("Bet") && html`<button onClick=${() => submit("bet")}>Bet ${!noLimit ? cents(minimum) : ""}</button>`}
    ${actions.has("Raise") && html`<button onClick=${() => submit("raise")}>Raise ${!noLimit ? cents(minimum) : ""}</button>`}
    ${actions.has("AllIn") && html`<button onClick=${() => submit("all_in")}>All in</button>`}
  </div>`;
}

function LastHand({ summary }) {
  if (!summary) return null;
  return html`<section class="card last-hand"><h2>Showdown</h2><div class="showdown-board">${summary.board.map((card) => html`<${Card} card=${card} />`)}</div>${summary.results.map((result) => html`<article class="showdown-row"><strong>Seat ${result.seat}</strong><span>${result.hand.label}</span><span>${summary.revealed_hole_cards.find(([seat]) => seat === result.seat)?.[1]?.join(" ") || ""}</span></article>`)}<p>${summary.awards.map((award) => html`<span>Seat ${award.seat} won ${cents(award.amount)}</span>`)}</p></section>`;
}

function TournamentPanel({ tournament }) {
  if (!tournament) return null;
  return html`<section class="card tournament-panel"><h2>Tournament</h2><p>Level ${tournament.level} · Blinds ${cents(tournament.small_blind)}/${cents(tournament.big_blind)} · Ante ${cents(tournament.ante)}</p><p>Hands at level: ${tournament.hands_at_level}/${tournament.hands_per_level}</p>${tournament.next_level && html`<p>Next level ${tournament.next_level}: ${cents(tournament.next_small_blind)}/${cents(tournament.next_big_blind)} · Ante ${cents(tournament.next_ante)}</p>`}<p>${tournament.finish_order.length ? `Finish order: ${tournament.finish_order.map((seat) => `Seat ${seat}`).join(", ")}` : "No eliminations yet"}</p></section>`;
}

function TableApp() {
  const [state, setState] = useState(null);
  const [joinSeat, setJoinSeat] = useState(null);
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
  const current = hand?.current_player == null ? null : state.seats.find((seat) => seat.index === hand.current_player);
  const currentName = current?.display_name || current?.occupant || "—";
  return html`<div class="table-shell">
    <div class="table-top"><h1>${state.name}</h1></div>
    <${TournamentPanel} tournament=${state.tournament} />
    <section class="felt" aria-label="Poker table">
      <div class="table-center"><p class="table-pot">${hand ? `Pot ${cents(hand.pot)}` : "Waiting for players"}</p><div class="board">${(hand?.board || []).map((card) => html`<${Card} card=${card} />`)}${Array.from({ length: 5 - (hand?.board?.length || 0) }).map(() => html`<${Card} empty />`)}</div><p class="table-status">${hand ? `${hand.street} · ${currentName}'s turn · To call ${cents(hand.to_call || 0)}` : "Waiting for players"}</p></div>
      <div class="seats">${state.seats.map((seat) => html`<${Seat} seat=${seat} total=${state.seats.length} button=${state.button} openSeat=${setJoinSeat} />`)}</div>
    </section>
    ${hand && html`<section class="hand-info"><div class="hole-cards">${(hand.your_hole_cards || []).map((card) => html`<${Card} card=${card} />`)}</div><${Actions} hand=${hand} tableId=${tableId} refresh=${refresh} /></section>`}
    <${LastHand} summary=${state.last_hand} />
    ${state.tournament?.finished ? null : html`<section class="card join-card"><h2>${state.tournament ? "Tournament registration" : "Join this table"}</h2>${state.tournament ? html`<p>Register through the lobby before the event starts.</p>` : html`<form onSubmit=${async (event) => { event.preventDefault(); const data = new FormData(event.currentTarget); await fetch(`/tables/${tableId}/join`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ seat: Number(data.get("seat")), buy_in: Math.round(Number(data.get("buy_in")) * 100) }) }); refresh(); }}><input name="buy_in" type="number" min="0.01" step="0.01" placeholder="Buy-in ($)" required /><input type="hidden" name="seat" value=${joinSeat ?? ""} /><button>Join seat ${joinSeat ?? "…"}</button></form>`}</section>`}
    <nav class="table-controls"><button onClick=${() => fetch(`/tables/${tableId}/sit`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ sitting_out: true }) }).then(refresh)}>Sit out</button><button onClick=${() => fetch(`/tables/${tableId}/leave`, { method: "POST" }).then(refresh)}>Leave</button></nav>
  </div>`;
}

if (root) render(html`<${TableApp} />`, root);
