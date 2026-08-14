import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings, useCardSettings } from "/public/card-settings.js";
import { responseError, wholeDollarMoney as money } from "/public/shared.js";
// Card geometry contracts live in card.js: rawRank === "T" ? "10", card-corner rank over suit.

const root = document.getElementById("table-app");
const tableId = root?.dataset.tableId;
const SHOWDOWN_PAUSE_MS = 6_000;
const FOLD_RESULT_PAUSE_MS = 3_000;

async function fetchState() {
  const response = await fetch(`/tables/${tableId}/state`, { headers: { Accept: "application/json" } });
  if (!response.ok) throw new Error("Unable to load table state");
  return response.json();
}

function actionName(action) {
  return typeof action === "string" ? action : Object.keys(action)[0];
}

function streetName(street) {
  return { Preflop: "Preflop", Flop: "Flop", Turn: "Turn", River: "River" }[street] || street;
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
  // Rail coordinates ride on custom properties so the mobile grid layout can
  // ignore them without fighting inline styles.
  return { "--seat-left": `${50 + x}%`, "--seat-top": `${50 + y}%` };
}

function Seat({ seat, player, events, current, button, order, total, viewer, viewerCards, showdown }) {
  const position = seatPosition(order, total);
  const tooltipBelow = Number.parseFloat(position["--seat-top"]) < 35;
  const positionLeft = Number.parseFloat(position["--seat-left"]);
  const tooltipHorizontal = positionLeft < 25 ? "tooltip-right" : positionLeft > 75 ? "tooltip-left" : null;
  const label = seat.display_name || seat.occupant;
  const role = blindRole(events, seat.index);
  const revealed = showdown?.revealed_hole_cards?.find(([seatIndex]) => seatIndex === seat.index)?.[1];
  const cards = revealed || (viewer ? viewerCards : player && !player.folded ? [null, null] : []);
  const winner = showdown?.awards?.some((award) => award.seat === seat.index);
  const classes = ["seat", viewer && "viewer", tooltipBelow && "tooltip-below", tooltipHorizontal, seat.index === button && "dealer", current && "acting", player?.folded && "folded", player?.all_in && "all-in", winner && "winner"].filter(Boolean).join(" ");
  return html`<article class=${classes} style=${position} data-seat-order=${order}>
    <span class="player-info" tabindex="0">
      <strong>${label}</strong><i aria-hidden="true">ⓘ</i>
      <span class="player-tooltip" role="tooltip"><b>Lifetime balance ${seat.bank_balance == null ? "Unavailable" : money(seat.bank_balance)}</b><span>Stack ${money(player?.stack ?? seat.stack)}</span>${seat.bank_entries.slice(-3).toReversed().map((entry) => html`<small>${entry.memo}: ${entry.delta >= 0 ? "+" : ""}${money(entry.delta)}</small>`)}</span>
    </span>
    <span class="seat-stack">${money(player?.stack ?? seat.stack)}</span>
    <span class="seat-badges">${role && html`<i class="seat-role">${role}</i>`}${current && html`<i class="seat-role acting-role">ACT</i>`}${player?.folded && html`<i class="seat-role state-role">FOLDED</i>`}${player?.all_in && html`<i class="seat-role state-role">ALL IN</i>`}${winner && html`<i class="seat-role winner-role">WINNER</i>`}</span>
    <span class=${`seat-wager ${player?.street_contribution > 0 ? "" : "no-wager"}`}>${money(player?.street_contribution || 0)}</span>
    ${cards.length > 0 && html`<span class=${`seat-cards ${revealed ? "revealed" : viewer ? "owned" : "hidden"}`}>${cards.map((card) => html`<${Card} card=${card} hidden=${card == null} interactive=${true} />`)}</span>`}
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
    // `amount` is the chips this action adds; the button shows the street
    // total it raises to, so it never reads the same as the call beside it.
    if (amount > 0 && !unique.has(amount)) unique.set(amount, { amount, total: contribution + amount, reason: candidate.reason });
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
  const wagerLabel = wagerKind === "bet" ? "Bet" : "Raise";
  return html`<div class="actions" aria-label="Actions">
    ${actions.has("Fold") && html`<button class="danger" onClick=${() => submit("fold")}>Fold</button>`}
    ${actions.has("Check") && html`<button class="primary-action" onClick=${() => submit("check")}>Check</button>`}
    ${actions.has("Call") && html`<button class="primary-action" onClick=${() => submit("call")}>Call ${money(hand.legal_actions.to_call)}</button>`}
    ${(actions.has("Bet") || actions.has("Raise")) && wagerOptions(hand).map((option) => html`<button class="wager-action" title=${`${wagerLabel} to ${money(option.total)} · ${option.reason}`} onClick=${() => submit(wagerKind, option.amount)}>${wagerLabel} ${money(option.total)}</button>`)}
    ${actions.has("AllIn") && html`<button class="wager-action all-in-action" onClick=${() => submit("all_in")}>All In</button>`}
    ${!hand.legal_actions.wager && hand.legal_actions.wagers_capped && html`<span class="capped-note">Betting capped · call or fold</span>`}
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

function ShowdownAdvance({ deadline, duration, canContinue, refresh }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(timer);
  }, []);
  const dueAt = Date.parse(deadline || "");
  const remaining = Number.isFinite(dueAt) ? Math.min(duration, Math.max(0, dueAt - now)) : duration;
  const seconds = Math.ceil(remaining / 1000);
  const width = `${(remaining / duration) * 100}%`;
  const label = `Next hand in ${seconds}s`;
  if (!canContinue) return html`<div class="showdown-advance spectator"><span class="showdown-progress" style=${{ width }}></span><b>${label}</b></div>`;
  return html`<div class="showdown-advance"><button type="button" aria-label=${`Continue now. ${label}`} onClick=${async () => {
    const response = await fetch(`/tables/${tableId}/continue`, { method: "POST" });
    if (response.ok) refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  }}><span class="showdown-progress" style=${{ width }}></span><b>OK · ${seconds}s</b></button></div>`;
}

function TableCommand({ state, openSeats, refresh }) {
  const viewer = state.viewer_seat == null
    ? null
    : state.seats.find((seat) => seat.index === state.viewer_seat);
  let label;
  let endpoint;
  let disabled = false;
  if (state.viewer_leaving) {
    label = "Leaving...";
    disabled = true;
  } else if (viewer && !state.tournament && viewer.stack <= 0 && !state.hand) {
    label = `Re-Buy In ${money(state.buy_in)}`;
    endpoint = `/tables/${tableId}/rebuy`;
  } else if (viewer) {
    label = "Leave";
    endpoint = `/tables/${tableId}/leave`;
  } else if (openSeats.length > 0 && (!state.tournament || (!state.tournament.started && !state.tournament.finished))) {
    label = `Buy In ${money(state.buy_in)}`;
    endpoint = state.tournament
      ? `/tournaments/${tableId}/register`
      : `/tables/${tableId}/join`;
  } else {
    return null;
  }
  const submit = async () => {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    if (response.ok) refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  };
  return html`<button class="table-command" type="button" disabled=${disabled} onClick=${submit}>${label}</button>`;
}

const seatBotDialog = () => document.getElementById("seat-bot");

function SeatBot({ state, openSeats, refresh }) {
  if (openSeats.length === 0 || state.tournament?.started) return null;
  const submit = async (event) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    const response = await fetch(`/tables/${tableId}/bot`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ seat: Number(data.get("seat")), kind: data.get("kind") }) });
    if (response.ok) {
      seatBotDialog()?.close();
      refresh();
    } else document.getElementById("table-error").textContent = await responseError(response);
  };
  return html`<span class="seat-bot">
    <button type="button" onClick=${() => seatBotDialog()?.showModal()}>Seat a bot</button>
    <dialog id="seat-bot" class="seat-bot-dialog">
      <form onSubmit=${submit}>
        <header><h2>Seat a bot</h2><button type="button" title="Cancel" aria-label="Cancel" onClick=${() => seatBotDialog()?.close()}>×</button></header>
        <label>Seat<select name="seat">${openSeats.map((seat) => html`<option value=${seat.index}>Seat ${seat.index}</option>`)}</select></label>
        <label>Bot kind<select name="kind"><option value="fish">fish</option><option value="rock">rock</option><option value="grinder">grinder</option><option value="shark">shark</option></select></label>
        <button type="submit">Seat bot</button>
      </form>
    </dialog>
  </span>`;
}

function TableApp() {
  const [state, setState] = useState(null);
  const [settings, setSettings] = useCardSettings();
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
  const resultPause = showdown?.revealed_hole_cards?.length > 1 ? SHOWDOWN_PAUSE_MS : FOLD_RESULT_PAUSE_MS;
  return html`<div class=${`table-shell ${settings.rankSize > 125 ? "compact-card-centers" : ""}`}>
    <${TournamentPanel} tournament=${state.tournament} />
    <section class="table-stage" aria-label="Poker table">
      <${CardSettings} settings=${settings} setSettings=${setSettings} interactive=${true} />
      <div class="felt">
        <div class="table-center">
          ${(hand || showdown) && html`<div class="table-metrics"><span><small>Pot</small><b>${money(hand?.pot || showdown?.awards?.reduce((sum, award) => sum + award.amount, 0) || 0)}</b></span>${hand && html`<span><small>Current bet</small><b>${money(hand.last_bet)}</b></span>`}</div>`}
          <div class="board">${board.map((card) => html`<${Card} card=${card} interactive=${true} />`)}</div>
          ${showdown && html`<p class="showdown-result">${result}</p>`}
        </div>
      </div>
      <div class="seats" data-seat-total=${ordered.length}>${ordered.map((seat, order) => html`<${Seat} seat=${seat} player=${hand?.players?.find((player) => player.seat === seat.index)} events=${hand?.events || showdown?.events || []} current=${hand?.current_player === seat.index} order=${order} total=${ordered.length} viewer=${seat.index === state.viewer_seat} viewerCards=${hand?.your_hole_cards || []} button=${state.button} showdown=${showdown} />`)}</div>
    </section>
    ${!showdown && (hand ? html`<p class="table-status">${streetName(hand.street)} · ${currentName} to act${hand.to_call ? ` · ${money(hand.to_call)} to call` : ""}</p>` : html`<p class="table-status waiting-status">Waiting for players</p>`)}
    ${(hand?.legal_actions || showdown) && html`<section class="decision-area">${showdown
      ? html`<${ShowdownAdvance} deadline=${state.next_hand_at} duration=${resultPause} canContinue=${state.viewer_seat != null} refresh=${refresh} />`
      : html`<${Actions} hand=${hand} tableId=${tableId} refresh=${refresh} />`}</section>`}
    ${handEvents.length > 0 && html`<${TableLog} events=${handEvents} seats=${state.seats} summary=${showdown} />`}
    <p id="table-error" class="error" role="alert"></p>
    <nav class="table-controls"><${SeatBot} state=${state} openSeats=${openSeats} refresh=${refresh} /><${TableCommand} state=${state} openSeats=${openSeats} refresh=${refresh} /></nav>
  </div>`;
}

if (root) render(html`<${TableApp} />`, root);
