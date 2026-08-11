import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
// Card geometry contracts live in card.js: rawRank === "T" ? "10", pip-grid-${value}, card-pip-${position}, card-art-${court}.

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

function money(value) {
  return `$${Math.round(value / 100).toLocaleString()}`;
}

function streetName(street) {
  return { Preflop: "Preflop", Flop: "Flop", Turn: "Turn", River: "River" }[street] || street;
}

function settingHandler(setter, key) {
  return (event) => {
    const value = Number(event.currentTarget.value);
    setter(value);
    localStorage.setItem(key, String(value));
  };
}

async function responseError(response) {
  const text = await response.text();
  const document = new DOMParser().parseFromString(text, "text/html");
  return document.querySelector("p")?.textContent?.trim() || text || `Request failed (${response.status})`;
}

function blindRole(events, seat) {
  if (events.some((event) => event.seat === seat && event.kind === "SmallBlind")) return "SB";
  if (events.some((event) => event.seat === seat && event.kind === "BigBlind")) return "BB";
  return null;
}

function seatPosition(order, total) {
  const halfWidth = 45;
  const halfHeight = 42;
  const perimeter = 4 * (halfWidth + halfHeight);
  let distance = (order / total) * perimeter;
  let x;
  let y;
  if (distance <= halfWidth) {
    x = -distance;
    y = halfHeight;
  } else if ((distance -= halfWidth) <= 2 * halfHeight) {
    x = -halfWidth;
    y = halfHeight - distance;
  } else if ((distance -= 2 * halfHeight) <= 2 * halfWidth) {
    x = -halfWidth + distance;
    y = -halfHeight;
  } else if ((distance -= 2 * halfWidth) <= 2 * halfHeight) {
    x = halfWidth;
    y = -halfHeight + distance;
  } else {
    distance -= 2 * halfHeight;
    x = halfWidth - distance;
    y = halfHeight;
  }
  return { left: `${50 + x}%`, top: `${50 + y}%`, transform: "translate(-50%, -50%)" };
}

function Seat({ seat, player, events, current, button, order, total, viewer, viewerCards, showdown }) {
  const position = seatPosition(order, total);
  const label = seat.display_name || seat.occupant;
  const role = blindRole(events, seat.index);
  const revealed = showdown?.revealed_hole_cards?.find(([seatIndex]) => seatIndex === seat.index)?.[1];
  const cards = revealed || (viewer ? viewerCards : player && !player.folded ? [null, null] : []);
  const winner = showdown?.awards?.some((award) => award.seat === seat.index);
  const classes = ["seat", viewer && "viewer", seat.index === button && "dealer", current && "acting", player?.folded && "folded", player?.all_in && "all-in", winner && "winner"].filter(Boolean).join(" ");
  return html`<article class=${classes} style=${position}>
    <span class="player-info" tabindex="0">
      <strong>${label}</strong><i aria-hidden="true">ⓘ</i>
      <span class="player-tooltip" role="tooltip"><b>Lifetime balance ${seat.bank_balance == null ? "Unavailable" : money(seat.bank_balance)}</b><span>Stack ${money(player?.stack ?? seat.stack)}</span>${seat.bank_entries.slice(-3).toReversed().map((entry) => html`<small>${entry.memo}: ${entry.delta >= 0 ? "+" : ""}${money(entry.delta)}</small>`)}</span>
    </span>
    <span class="seat-stack">${money(player?.stack ?? seat.stack)}</span>
    <span class="seat-badges">${role && html`<i class="seat-role">${role}</i>`}${current && html`<i class="seat-role acting-role">ACT</i>`}${player?.folded && html`<i class="seat-role state-role">FOLDED</i>`}${player?.all_in && html`<i class="seat-role state-role">ALL IN</i>`}${winner && html`<i class="seat-role winner-role">WINNER</i>`}</span>
    ${player?.street_contribution > 0 && html`<span class="seat-wager">${money(player.street_contribution)}</span>`}
    ${cards.length > 0 && html`<span class=${`seat-cards ${revealed ? "revealed" : viewer ? "owned" : "hidden"}`}>${cards.map((card) => html`<${Card} card=${card} hidden=${card == null} />`)}</span>`}
    ${seat.index === button && html`<i class="button-marker">D</i>`}
  </article>`;
}

function wagerOptions(hand) {
  const wager = hand?.legal_actions?.wager;
  if (!wager) return [];
  const player = hand.players.find((candidate) => candidate.seat === hand.legal_actions.seat);
  const contribution = player?.street_contribution || 0;
  const candidates = wager.fixed != null
    ? [{ amount: wager.fixed, reason: "Fixed wager" }]
    : [
        { amount: wager.min, reason: "Minimum" },
        { amount: hand.last_bet * 2 - contribution, reason: "Double current bet" },
        { amount: hand.pot / 2, reason: "Half pot" },
        { amount: hand.pot, reason: "Pot" },
      ];
  const unique = new Map();
  for (const candidate of candidates) {
    const rounded = Math.round(candidate.amount / 100) * 100;
    const amount = Math.max(wager.min, Math.min(wager.max, rounded));
    if (amount > 0 && !unique.has(amount)) unique.set(amount, { amount, reason: candidate.reason });
  }
  return [...unique.values()].toSorted((left, right) => left.amount - right.amount);
}

function Actions({ hand, tableId: actionTableId, refresh }) {
  const actions = new Set((hand?.legal_actions?.actions || []).map(actionName));
  const submit = async (kind, amount) => {
    const response = await fetch(`/tables/${actionTableId}/action`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ kind, amount }) });
    if (response.ok) refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  };
  const wagerKind = actions.has("Bet") ? "bet" : "raise";
  return html`<div class="actions" aria-label="Actions">
    ${actions.has("Fold") && html`<button class="danger" onClick=${() => submit("fold")}>Fold</button>`}
    ${actions.has("Check") && html`<button class="primary-action" onClick=${() => submit("check")}>Check</button>`}
    ${actions.has("Call") && html`<button class="primary-action" onClick=${() => submit("call")}>Call ${money(hand.legal_actions.to_call)}</button>`}
    ${(actions.has("Bet") || actions.has("Raise")) && wagerOptions(hand).map((option) => html`<button class="wager-action" title=${option.reason} onClick=${() => submit(wagerKind, option.amount)}>${money(option.amount)}</button>`)}
    ${actions.has("AllIn") && html`<button class="wager-action all-in-action" onClick=${() => submit("all_in")}>All In</button>`}
  </div>`;
}

function TournamentPanel({ tournament }) {
  if (!tournament) return null;
  return html`<section class="tournament-panel"><b>Level ${tournament.level}</b><span>Blinds ${money(tournament.small_blind)}/${money(tournament.big_blind)}</span><span>Ante ${money(tournament.ante)}</span><span>${tournament.hands_at_level}/${tournament.hands_per_level} hands</span></section>`;
}

function eventLabel(event, seats) {
  const seat = event.seat == null ? null : seats.find((candidate) => candidate.index === event.seat);
  const name = seat?.display_name || seat?.occupant || `Seat ${event.seat}`;
  const amount = event.amount > 0 ? ` ${money(event.amount)}` : "";
  return {
    Ante: `${name} posts ante${amount}`,
    SmallBlind: `${name} posts small blind${amount}`,
    BigBlind: `${name} posts big blind${amount}`,
    Fold: `${name} folds`,
    Check: `${name} checks`,
    Call: `${name} calls${amount}`,
    Bet: `${name} bets${amount}`,
    Raise: `${name} raises${amount}`,
    AllIn: `${name} is all in${amount}`,
    Deal: `${streetName(event.street)} dealt`,
    Award: `${name} wins${amount}`,
  }[event.kind] || event.kind;
}

function winnerLines(summary, seats) {
  if (!summary) return [];
  const totals = new Map();
  for (const award of summary.awards) totals.set(award.seat, (totals.get(award.seat) || 0) + award.amount);
  return [...totals.entries()].map(([seatIndex, amount]) => {
    const seat = seats.find((candidate) => candidate.index === seatIndex);
    const result = summary.results.find((candidate) => candidate.seat === seatIndex);
    return `${seat?.display_name || seat?.occupant || "Player"} wins ${money(amount)}${result?.hand?.label ? ` with ${result.hand.label}` : ""}`;
  });
}

function TableLog({ events, seats, summary }) {
  const results = winnerLines(summary, seats);
  return html`<section class="game-log" aria-live="polite"><h2>Table log</h2><ol>${results.map((result) => html`<li class="result-log"><span>Result</span><b>${result}</b></li>`)}${events.slice(-16).toReversed().map((event) => html`<li><span>${streetName(event.street)}</span><b>${eventLabel(event, seats)}</b></li>`)}</ol></section>`;
}

function TableApp() {
  const [state, setState] = useState(null);
  const [cardScale, setCardScale] = useState(() => Number(localStorage.getItem("table-card-scale")) || 180);
  const [rankScale, setRankScale] = useState(() => Number(localStorage.getItem("table-rank-scale")) || 130);
  const [rankWeight, setRankWeight] = useState(() => Number(localStorage.getItem("table-rank-weight")) || 850);
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
  const showdown = hand ? null : state.last_hand;
  const handEvents = hand?.events || showdown?.events || [];
  const current = hand?.current_player == null ? null : state.seats.find((seat) => seat.index === hand.current_player);
  const currentName = current?.display_name || current?.occupant || "—";
  const occupied = state.seats.filter((seat) => seat.occupant !== "empty");
  const viewerOffset = Math.max(0, occupied.findIndex((seat) => seat.index === state.viewer_seat));
  const ordered = [...occupied.slice(viewerOffset), ...occupied.slice(0, viewerOffset)];
  const board = hand?.board || showdown?.board || [];
  const openSeats = state.seats.filter((seat) => seat.occupant === "empty");
  const result = winnerLines(showdown, state.seats).join(" · ");
  const scale = cardScale / 100;
  const cardStyle = {
    "--viewer-card-scale": cardScale,
    "--viewer-card-w": `${3 * scale}rem`,
    "--viewer-card-h": `${4.2 * scale}rem`,
    "--viewer-corner": `${0.52 * scale}rem`,
    "--viewer-pip": `${0.68 * scale}rem`,
    "--viewer-art": `${1.7 * scale}rem`,
    "--viewer-card-w-mobile": `${2.1 * scale}rem`,
    "--viewer-card-h-mobile": `${2.95 * scale}rem`,
    "--card-rank-scale": rankScale / 100,
    "--card-rank-weight": rankWeight,
  };
  return html`<div class="table-shell" style=${cardStyle}>
    <${TournamentPanel} tournament=${state.tournament} />
    <section class="table-stage" aria-label="Poker table">
      <button class="table-config-button" type="button" title="Card display settings" aria-label="Card display settings" onClick=${() => document.getElementById("card-config")?.showModal()}>⚙</button>
      <dialog id="card-config" class="card-config-dialog">
        <form method="dialog">
          <header><h2>Card display</h2><button type="submit" title="Close" aria-label="Close">×</button></header>
          <label><span>Card size <output>${cardScale}%</output></span><input name="card-scale" type="range" min="80" max="180" step="5" value=${cardScale} onInput=${settingHandler(setCardScale, "table-card-scale")} /></label>
          <label><span>Rank size <output>${rankScale}%</output></span><input name="rank-scale" type="range" min="100" max="150" step="5" value=${rankScale} onInput=${settingHandler(setRankScale, "table-rank-scale")} /></label>
          <label><span>Rank weight <output>${rankWeight}</output></span><input name="rank-weight" type="range" min="600" max="900" step="50" value=${rankWeight} onInput=${settingHandler(setRankWeight, "table-rank-weight")} /></label>
        </form>
      </dialog>
      <div class="felt">
        <div class="table-center">
          ${(hand || showdown) && html`<div class="table-metrics"><span><small>Pot</small><b>${money(hand?.pot || showdown?.awards?.reduce((sum, award) => sum + award.amount, 0) || 0)}</b></span>${hand && html`<span><small>Current bet</small><b>${money(hand.last_bet)}</b></span>`}</div>`}
          <div class="board">${board.map((card) => html`<${Card} card=${card} />`)}${Array.from({ length: 5 - board.length }).map(() => html`<${Card} empty />`)}</div>
          ${showdown ? html`<p class="showdown-result">${result}</p>` : hand ? html`<p class="table-status">${streetName(hand.street)} · ${currentName} to act${hand.to_call ? ` · ${money(hand.to_call)} to call` : ""}</p>` : html`<p class="table-status waiting-status">Waiting for players</p>`}
        </div>
      </div>
      <div class="seats">${ordered.map((seat, order) => html`<${Seat} seat=${seat} player=${hand?.players?.find((player) => player.seat === seat.index)} events=${hand?.events || showdown?.events || []} current=${hand?.current_player === seat.index} order=${order} total=${ordered.length} viewer=${seat.index === state.viewer_seat} viewerCards=${hand?.your_hole_cards || []} button=${state.button} showdown=${showdown} />`)}</div>
    </section>
    ${hand?.legal_actions && html`<section class="decision-area"><${Actions} hand=${hand} tableId=${tableId} refresh=${refresh} /></section>`}
    ${handEvents.length > 0 && html`<${TableLog} events=${handEvents} seats=${state.seats} summary=${showdown} />`}
    ${state.viewer_seat == null && !state.tournament && openSeats.length > 0 && html`<section class="card join-card"><h2>Buy in · ${money(state.buy_in)}</h2><form onSubmit=${async (event) => { event.preventDefault(); const seat = Number(new FormData(event.currentTarget).get("seat")); const response = await fetch(`/tables/${tableId}/join`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ seat }) }); if (response.ok) refresh(); else document.getElementById("table-error").textContent = await responseError(response); }}><label>Seat<select name="seat">${openSeats.map((seat) => html`<option value=${seat.index}>Seat ${seat.index}</option>`)}</select></label><button>Buy in</button></form></section>`}
    <p id="table-error" class="error" role="alert"></p>
    ${state.tournament && !state.tournament.finished && state.viewer_seat == null && html`<section class="card join-card"><h2>Register for tournament</h2><form onSubmit=${async (event) => { event.preventDefault(); const data = new FormData(event.currentTarget); const response = await fetch(`/tournaments/${tableId}/register`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ seat: Number(data.get("seat")), buy_in: Math.round(Number(data.get("buy_in")) * 100) }) }); if (response.ok) refresh(); else document.getElementById("table-error").textContent = await responseError(response); }}><label>Seat<select name="seat">${openSeats.map((seat) => html`<option value=${seat.index}>Seat ${seat.index}</option>`)}</select></label><label>Buy-in ($)<input name="buy_in" type="number" min="1" max="10000" step="1" required /></label><button>Register</button></form></section>`}
    ${(!state.tournament || !state.tournament.started) && state.seats.some((seat) => seat.occupant === "empty") && html`<section class="card bot-card"><h2>Seat a bot</h2><form onSubmit=${async (event) => { event.preventDefault(); const data = new FormData(event.currentTarget); const response = await fetch(`/tables/${tableId}/bot`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ seat: Number(data.get("seat")), kind: data.get("kind") }) }); if (response.ok) refresh(); else document.getElementById("table-error").textContent = await responseError(response); }}><label>Seat<select name="seat">${state.seats.filter((seat) => seat.occupant === "empty").map((seat) => html`<option value=${seat.index}>Seat ${seat.index}</option>`)}</select></label><label>Bot kind<select name="kind"><option value="fish">fish</option><option value="rock">rock</option><option value="grinder">grinder</option><option value="shark">shark</option></select></label><button>Seat bot</button></form></section>`}
    <nav class="table-controls"><button onClick=${() => fetch(`/tables/${tableId}/sit`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ sitting_out: true }) }).then(refresh)}>Sit out</button><button onClick=${() => fetch(`/tables/${tableId}/leave`, { method: "POST" }).then(refresh)}>Leave</button></nav>
  </div>`;
}

if (root) render(html`<${TableApp} />`, root);
