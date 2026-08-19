import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings, useCardSettings } from "/public/card-settings.js";
import { responseError, wholeDollarMoney as money } from "/public/shared.js";
// Card geometry contracts live in card.js: rawRank === "T" ? "10", card-corner rank over suit.

const root = document.getElementById("table-app");
const tableId = root?.dataset.tableId;
const SHOWDOWN_PAUSE_MS = 6_000;
const FOLD_RESULT_PAUSE_MS = 3_000;
const RUNOUT_STEP_MS = 5_000;

// An all-in board arrives a street at a time. Elapsed time since the hand
// ended decides how much of it is face up and who is currently ahead.
function runoutState(showdown, elapsed) {
  const steps = showdown?.runout || [];
  if (steps.length === 0) return { cards: showdown?.board?.length ?? 0, leaders: [] };
  const taken = Math.min(steps.length, Math.floor(elapsed / RUNOUT_STEP_MS));
  const step = taken > 0 ? steps[taken - 1] : null;
  return {
    cards: step ? step.cards : showdown.runout_from ?? 0,
    // Somebody is ahead the moment the hands are turned over, not only once a
    // card has landed on top of them.
    leaders: step?.leaders || showdown.reveal_leaders || [],
  };
}

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

function Seat({ seat, player, events, current, button, order, total, viewer, viewerCards, showdown, leading, settled }) {
  const position = seatPosition(order, total);
  const seatTop = Number.parseFloat(position["--seat-top"]);
  const tooltipBelow = seatTop < 35;
  // Cards hang toward the middle of the table; hanging outward would push a
  // bottom seat's hand off the stage entirely.
  const cardsAbove = seatTop > 55;
  const positionLeft = Number.parseFloat(position["--seat-left"]);
  const tooltipHorizontal = positionLeft < 25 ? "tooltip-right" : positionLeft > 75 ? "tooltip-left" : null;
  const label = seat.display_name || seat.occupant;
  const role = blindRole(events, seat.index);
  const revealed = showdown?.revealed_hole_cards?.find(([seatIndex]) => seatIndex === seat.index)?.[1];
  const cards = revealed || (viewer ? viewerCards : player && !player.folded ? [null, null] : []);
  // While a board is still running out, nobody has won anything yet.
  const awarded = showdown?.awards
    ?.filter((award) => award.seat === seat.index)
    .reduce((sum, award) => sum + award.amount, 0) || 0;
  // Stacks settle when the hand does. Until the last card is down they read as
  // the hand left them, or the balance gives the result away. That figure comes
  // from the hand itself: the live seat has moved on since.
  const beforeAwards = showdown?.stacks_before_awards?.[seat.index];
  const stack = settled || beforeAwards == null
    ? (player?.stack ?? seat.stack)
    : beforeAwards;
  const winner = settled && awarded > 0;
  const classes = ["seat", viewer && "viewer", tooltipBelow && "tooltip-below", cardsAbove && "cards-above", tooltipHorizontal, seat.index === button && "dealer", current && "acting", player?.folded && "folded", player?.all_in && "all-in", leading && "leading", winner && "winner"].filter(Boolean).join(" ");
  return html`<article class=${classes} style=${position} data-seat-order=${order}>
    <span class="seat-corner-badges">${seat.index === button && html`<i class="seat-role button-role">D</i>`}${role && html`<i class="seat-role">${role}</i>`}</span>
    <span class="player-info" tabindex="0">
      <strong>${label}</strong><i aria-hidden="true">ⓘ</i>
      <span class="player-tooltip" role="tooltip"><b>Lifetime balance ${seat.bank_balance == null ? "Unavailable" : money(seat.bank_balance)}</b><span>Stack ${money(stack)}</span>${seat.bank_entries.slice(-3).toReversed().map((entry) => html`<small>${entry.memo}: ${entry.delta >= 0 ? "+" : ""}${money(entry.delta)}</small>`)}</span>
    </span>
    <span class="seat-stack">${money(stack)}</span>
    <span class="seat-badges"></span>
    ${player?.folded && !viewer
      ? html`<span class="seat-card-state"><i class="seat-role state-role">FOLDED</i></span>`
      : cards.length > 0 && html`<span class=${`seat-cards ${revealed ? "revealed" : viewer ? "owned" : "hidden"}`}>${cards.map((card) => html`<${Card} card=${card} hidden=${card == null} interactive=${viewer} />`)}</span>`}
    <span class=${`seat-wager ${player?.street_contribution > 0 || player?.all_in ? "" : "no-wager"}`}>${player?.all_in ? "ALL IN" : money(player?.street_contribution || 0)}</span>
    <span class="seat-outcome-badges">${leading && html`<i class="seat-role leading-role">AHEAD</i>`}${winner && html`<i class="seat-role winner-role">WINNER</i>`}</span>
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

// With nobody sitting down, the house waits to be asked for a hand.
function DealHouseHand({ refresh }) {
  return html`<div class="showdown-advance house-deal"><button type="button" onClick=${async () => {
    const response = await fetch(`/tables/${tableId}/deal`, { method: "POST" });
    if (response.ok) refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  }}><b>Deal a hand</b></button></div>`;
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

function TableLog({ events, seats, summary, settled }) {
  const results = settled ? winnerLines(summary, seats) : [];
  // Awards are the punchline; they wait for the last card like everything else.
  const shown = settled ? events : events.filter((event) => event.kind !== "Award");
  return html`<section class="game-log" aria-live="polite"><h2>Table log</h2><ol>${results.map((result) => html`<li class="result-log"><span>Result</span><b>${result}</b></li>`)}${shown.slice(-16).toReversed().map((event) => html`<li><span>${streetName(event.street)}</span><b>${eventLabel(event, seats)}</b></li>`)}</ol></section>`;
}

// One clock for the whole result: it paces the runout and the countdown.
function useResultClock(active, deadline, duration) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!active) return undefined;
    const timer = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(timer);
  }, [active, deadline]);
  const dueAt = Date.parse(deadline || "");
  return Number.isFinite(dueAt) ? Math.min(duration, Math.max(0, dueAt - now)) : duration;
}

function ShowdownAdvance({ remaining, duration, canContinue, refresh }) {
  const seconds = Math.ceil(remaining / 1000);
  const width = `${(remaining / duration) * 100}%`;
  const label = `Next hand in ${seconds}s`;
  // A board still running out is not skippable, so offer no button to press.
  if (!canContinue) return html`<div class="showdown-advance spectator"><span class="showdown-progress" style=${{ width }}></span><b>${label}</b></div>`;
  return html`<div class="showdown-advance"><button type="button" aria-label=${`Continue now. ${label}`} onClick=${async () => {
    const response = await fetch(`/tables/${tableId}/continue`, { method: "POST" });
    if (response.ok) refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  }}><span class="showdown-progress" style=${{ width }}></span><b>OK · ${seconds}s</b></button></div>`;
}

const forfeitDialog = () => document.getElementById("forfeit-entry");

function TableCommand({ label, endpoint, disabled, forfeits, buyIn, refresh }) {
  const submit = async () => {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    if (response.ok) refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  };
  // Walking out of a tournament is not a cash-out: the entry is gone, so ask first.
  if (forfeits) {
    return html`<span class="table-command-confirm">
      <button class="table-command" type="button" onClick=${() => forfeitDialog()?.showModal()}>${label}</button>
      <dialog id="forfeit-entry" class="confirm-dialog">
        <form method="dialog">
          <header><h2>Leave the tournament?</h2></header>
          <p>You forfeit your entry. The ${money(buyIn)} buy-in stays in the prize pool, your chips leave the table, and you finish in your current place.</p>
          <footer>
            <button type="submit" value="stay">Keep playing</button>
            <button class="danger" type="button" onClick=${() => { forfeitDialog()?.close(); submit(); }}>Forfeit and leave</button>
          </footer>
        </form>
      </dialog>
    </span>`;
  }
  return html`<button class="table-command" type="button" disabled=${disabled} onClick=${submit}>${label}</button>`;
}

/// What the viewer can do about their seat. Busted at a cash table you get two
/// choices, because rebuying must never be the only way out.
function TableCommands({ state, openSeats, refresh }) {
  // A table full of house players still has room: you take one of their seats.
  const seatsForYou = state.tournament
    ? openSeats
    : [...openSeats, ...state.seats.filter((seat) => seat.bot)];
  const canAffordCashSeat = state.tournament || (state.bank_balance ?? 0) >= state.buy_in;
  const viewer = state.viewer_seat == null
    ? null
    : state.seats.find((seat) => seat.index === state.viewer_seat);
  const leave = {
    label: "Leave",
    endpoint: `/tables/${tableId}/leave`,
    forfeits: Boolean(state.tournament),
  };
  const commands = [];
  if (state.viewer_leaving) {
    commands.push({ label: "Leaving...", disabled: true });
  } else if (viewer) {
    if (!state.tournament && viewer.stack <= 0 && !state.hand) {
      commands.push({ label: `Re-Buy In ${money(state.buy_in)}`, endpoint: `/tables/${tableId}/rebuy`, disabled: !canAffordCashSeat });
    }
    commands.push(leave);
  } else if (seatsForYou.length > 0 && (!state.tournament || (!state.tournament.started && !state.tournament.finished))) {
    commands.push({
      label: `Buy In ${money(state.buy_in)}`,
      endpoint: state.tournament ? `/tournaments/${tableId}/register` : `/tables/${tableId}/join`,
      disabled: !canAffordCashSeat,
    });
  }
  return commands.map((command) => html`<${TableCommand} ...${command} buyIn=${state.buy_in} refresh=${refresh} />`);
}

function SeatBot({ state, openSeats, refresh }) {
  if (openSeats.length === 0 || state.tournament?.started) return null;
  const submit = async (kind) => {
    const response = await fetch(`/tables/${tableId}/bot`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ kind }) });
    if (response.ok) {
      refresh();
    } else document.getElementById("table-error").textContent = await responseError(response);
  };
  return html`<span class="seat-bot" aria-label="Seat a bot">
    ${["fish", "rock", "grinder", "shark"].map((kind) => html`<button type="button" onClick=${() => submit(kind)}>Seat ${kind}</button>`)}
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
  const showdown = state && !state.hand ? state.last_hand : null;
  const resultPause = 1000 * (state?.result_pause_seconds
    ?? (showdown?.revealed_hole_cards?.length > 1 ? SHOWDOWN_PAUSE_MS : FOLD_RESULT_PAUSE_MS) / 1000);
  const remaining = useResultClock(Boolean(showdown), state?.next_hand_at, resultPause);
  if (!state) return html`<p class="loading">Loading table…</p>`;
  const hand = state.hand;
  const handEvents = hand?.events || showdown?.events || [];
  const current = hand?.current_player == null ? null : state.seats.find((seat) => seat.index === hand.current_player);
  const currentName = current?.display_name || current?.occupant || "—";
  const occupied = state.seats.filter((seat) => seat.occupant !== "empty");
  const viewerOffset = Math.max(0, occupied.findIndex((seat) => seat.index === state.viewer_seat));
  const ordered = [...occupied.slice(viewerOffset), ...occupied.slice(0, viewerOffset)];
  const viewerSeat = ordered.find((seat) => seat.index === state.viewer_seat);
  const otherSeats = ordered.filter((seat) => seat.index !== state.viewer_seat);
  const runout = runoutState(showdown, resultPause - remaining);
  const board = hand?.board || (showdown ? showdown.board.slice(0, runout.cards) : []);
  const openSeats = state.seats.filter((seat) => seat.occupant === "empty");
  // The result only reads once the whole board is out.
  const settled = runout.cards >= (showdown?.board?.length ?? 0);
  // Nobody is seated, so no next hand is coming on its own: once the result
  // has had its moment, hand the table back to whoever is watching.
  const awaitingDeal = state.can_deal && (!showdown || remaining <= 0);
  const result = settled ? winnerLines(showdown, state.seats).join(" · ") : "";
  const renderSeat = (seat, order, seats) => html`<${Seat} seat=${seat} player=${hand?.players?.find((player) => player.seat === seat.index)} events=${hand?.events || showdown?.events || []} current=${hand?.current_player === seat.index} order=${order} total=${seats.length} viewer=${seat.index === state.viewer_seat} viewerCards=${hand?.your_hole_cards || []} button=${state.button} showdown=${showdown} leading=${runout.leaders.includes(seat.index)} settled=${settled} />`;
  return html`<div class=${`table-shell ${settings.paranoid ? "paranoid-cards" : ""}`}>
    <${TournamentPanel} tournament=${state.tournament} />
    <p class=${`table-status ${hand ? "" : "waiting-status"}`}>${showdown
      ? ""
      : hand
        ? `${streetName(hand.street)} · ${currentName} to act${hand.to_call ? ` · ${money(hand.to_call)} to call` : ""}`
        : state.can_deal
          ? "Nobody seated · deal a hand"
          : "Waiting for players"}</p>
    <section class="table-stage" aria-label="Poker table">
      <${CardSettings} settings=${settings} setSettings=${setSettings} interactive=${true} concealable=${true} />
      <div class="seats other-seats" data-seat-total=${otherSeats.length}>${otherSeats.map((seat, order) => renderSeat(seat, order, otherSeats))}</div>
      <div class="felt">
        <div class="table-center">
          ${(hand || showdown) && html`<div class="table-metrics"><span><small>Pot</small><b>${money(hand?.pot || showdown?.awards?.reduce((sum, award) => sum + award.amount, 0) || 0)}</b></span>${hand && html`<span><small>Current bet</small><b>${money(hand.last_bet)}</b></span>`}</div>`}
          <div class="board">${board.map((card) => html`<${Card} card=${card} interactive=${true} />`)}</div>
          ${showdown && html`<p class="showdown-result">${result}</p>`}
        </div>
      </div>
      ${viewerSeat && html`<div class="seats viewer-seats" data-seat-total="1">${renderSeat(viewerSeat, 0, [viewerSeat])}</div>`}
    </section>
    <section class="decision-area">${showdown && !awaitingDeal
      ? html`<${ShowdownAdvance} remaining=${remaining} duration=${resultPause} canContinue=${settled && state.viewer_seat != null} refresh=${refresh} />`
      : hand?.legal_actions
        ? html`<${Actions} hand=${hand} tableId=${tableId} refresh=${refresh} />`
        : state.can_deal
          ? html`<${DealHouseHand} refresh=${refresh} />`
          : null}</section>
    <${TableLog} events=${handEvents} seats=${state.seats} summary=${showdown} settled=${settled} />
    <p id="table-error" class="error" role="alert"></p>
    <nav class="table-controls"><a class="table-history-link" href=${`/tables/${tableId}/history`}>History</a><${SeatBot} state=${state} openSeats=${openSeats} refresh=${refresh} /><${TableCommands} state=${state} openSeats=${openSeats} refresh=${refresh} /></nav>
  </div>`;
}

if (root) render(html`<${TableApp} />`, root);
