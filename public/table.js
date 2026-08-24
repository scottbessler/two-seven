import { html, render, useEffect, useRef, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings, useCardSettings } from "/public/card-settings.js";
import { refreshBank, responseError, wholeDollarMoney as money } from "/public/shared.js";
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
  if (steps.length === 0) return { cards: showdown?.board?.length ?? 0, leaders: [], odds: showdown?.reveal_odds || [] };
  const taken = Math.min(steps.length, Math.floor(elapsed / RUNOUT_STEP_MS));
  const step = taken > 0 ? steps[taken - 1] : null;
  return {
    cards: step ? step.cards : showdown.runout_from ?? 0,
    // Somebody is ahead the moment the hands are turned over, not only once a
    // card has landed on top of them.
    leaders: step?.leaders || showdown.reveal_leaders || [],
    odds: step?.odds || showdown.reveal_odds || [],
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

function Seat({ seat, player, events, current, button, viewer, viewerCards, showdown, leading, settled, champion }) {
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
  const classes = ["seat", viewer && "viewer", seat.index === button && "dealer", current && "acting", player?.folded && "folded", player?.all_in && "all-in", leading && "leading", winner && "winner", champion && "champion"].filter(Boolean).join(" ");
  const playerInfo = html`<span class="player-info" tabindex="0">
    <strong>${label}</strong><i aria-hidden="true">ⓘ</i>
    <span class="player-tooltip" role="tooltip"><b>Lifetime balance ${seat.bank_balance == null ? "Unavailable" : money(seat.bank_balance)}</b><span>Stack ${money(stack)}</span>${seat.bank_entries.slice(-3).toReversed().map((entry) => html`<small>${entry.memo}: ${entry.delta >= 0 ? "+" : ""}${money(entry.delta)}</small>`)}</span>
  </span>`;
  const stackLabel = html`<span class="seat-stack">${money(stack)}</span>`;
  const wager = html`<span class=${`seat-wager ${player?.street_contribution > 0 || player?.all_in ? "" : "no-wager"}`}>${player?.all_in ? "ALL IN" : money(player?.street_contribution || 0)}</span>`;
  return html`<article class=${classes}>
    <span class="seat-corner-badges">${seat.index === button && html`<i class="seat-role button-role">D</i>`}${role && html`<i class="seat-role">${role}</i>`}</span>
    ${viewer ? html`<span class="viewer-summary">${playerInfo}${stackLabel}${wager}</span>` : html`${playerInfo}${stackLabel}`}
    ${player?.folded && !viewer
      ? html`<span class="seat-card-state"><i class="seat-role state-role">FOLDED</i></span>`
      : cards.length > 0 && html`<span class=${`seat-cards ${revealed ? "revealed" : viewer ? "owned" : "hidden"}`}>${cards.map((card) => html`<${Card} card=${card} hidden=${card == null} interactive=${viewer} />`)}</span>`}
    ${!viewer && wager}
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

function HoldAction({ label, className, hold, submit, ariaLabel }) {
  const holdSeconds = 1;
  const timer = useRef(null);
  const [holding, setHolding] = useState(false);
  const cancel = () => {
    if (timer.current != null) clearTimeout(timer.current);
    timer.current = null;
    setHolding(false);
  };
  useEffect(() => () => {
    if (timer.current != null) clearTimeout(timer.current);
  }, []);
  const start = (event) => {
    if (!hold || timer.current != null) return;
    if (event.type === "keydown" && !["Enter", " "].includes(event.key)) return;
    event.preventDefault();
    setHolding(true);
    timer.current = setTimeout(() => {
      timer.current = null;
      setHolding(false);
      submit();
    }, holdSeconds * 1_000);
  };
  const stop = (event) => {
    if (event.type === "keyup" && !["Enter", " "].includes(event.key)) return;
    if (hold) cancel();
  };
  return html`<button
    class=${`${className} ${hold ? "hold-action" : ""} ${holding ? "holding" : ""}`}
    type="button"
    aria-label=${hold ? `Hold ${ariaLabel || label} for ${holdSeconds} second` : ariaLabel}
    title=${hold ? `Hold for ${holdSeconds} second` : undefined}
    onClick=${hold ? (event) => event.preventDefault() : submit}
    onContextMenu=${(event) => event.preventDefault()}
    onSelectStart=${(event) => event.preventDefault()}
    onDragStart=${(event) => event.preventDefault()}
    onPointerDown=${start}
    onPointerUp=${stop}
    onPointerLeave=${stop}
    onPointerCancel=${stop}
    onKeyDown=${start}
    onKeyUp=${stop}
    onBlur=${cancel}
  ><span>${label}</span></button>`;
}

function Actions({ hand, seats, tableId: actionTableId, settings, refresh }) {
  const actions = new Set((hand?.legal_actions?.actions || []).map(actionName));
  const submit = async (kind, amount) => {
    const response = await fetch(`/tables/${actionTableId}/action`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ kind, amount }) });
    if (response.ok) refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  };
  const wagerKind = actions.has("Bet") ? "bet" : "raise";
  const wagerLabel = wagerKind === "bet" ? "Bet" : "Raise";
  const actor = seats.find((seat) => seat.index === hand.legal_actions.seat);
  const callIsAllIn = Boolean(actor && actions.has("Call") && (hand.legal_actions.to_call || 0) >= actor.stack);
  const wagers = wagerOptions(hand).filter((option) => !actor || option.amount < actor.stack);
  const middleCount = Number(actions.has("Check")) + Number(actions.has("Call") && !callIsAllIn) + wagers.length;
  const showAllIn = actions.has("AllIn") || callIsAllIn;
  return html`<div class="actions" aria-label="Actions">
    <span class="action-edge action-edge-left">${actions.has("Fold") && html`<${HoldAction} label="Fold" className="danger fold-action" hold=${settings.confirmFold} submit=${() => submit("fold")} />`}</span>
    <span class="action-middle" style=${`--middle-action-count:${Math.max(1, middleCount)}`}>
      ${actions.has("Check") && html`<button class="primary-action" onClick=${() => submit("check")}><span>Check</span></button>`}
      ${actions.has("Call") && !callIsAllIn && html`<button class="primary-action" aria-label=${`Call ${money(hand.legal_actions.to_call)}`} onClick=${() => submit("call")}><span class="action-prefix">Call </span><span>${money(hand.legal_actions.to_call)}</span></button>`}
      ${(actions.has("Bet") || actions.has("Raise")) && wagers.map((option) => html`<button class="wager-action" aria-label=${`${wagerLabel} ${money(option.total)}`} title=${`${wagerLabel} to ${money(option.total)} · ${option.reason}`} onClick=${() => submit(wagerKind, option.amount)}><span class="action-prefix">${wagerLabel} </span><span>${money(option.total)}</span></button>`)}
    </span>
    <span class="action-edge action-edge-right">${showAllIn && html`<${HoldAction} label="All In" className="wager-action all-in-action" hold=${settings.confirmAllIn} submit=${() => submit(callIsAllIn ? "call" : "all_in")} />`}</span>
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

function tournamentInfoText(tournament) {
  if (!tournament) return "";
  return `Level ${tournament.level} · Blinds ${money(tournament.small_blind)}/${money(tournament.big_blind)} · Ante ${money(tournament.ante)} · ${tournament.hands_at_level}/${tournament.hands_per_level} hands`;
}

function useHeaderInfo(tournament) {
  const info = tournamentInfoText(tournament);
  useEffect(() => {
    const context = document.querySelector(".header-context");
    if (!context) return undefined;
    context.querySelector(".header-info")?.remove();
    if (!info) return undefined;
    const trigger = document.createElement("span");
    trigger.className = "header-info";
    trigger.tabIndex = 0;
    trigger.setAttribute("aria-label", info);
    trigger.textContent = "ⓘ";
    const tooltip = document.createElement("span");
    tooltip.className = "header-info-tooltip";
    tooltip.role = "tooltip";
    tooltip.textContent = info;
    trigger.append(tooltip);
    context.append(" ", trigger);
    return () => trigger.remove();
  }, [info]);
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

function cardText(card) {
  const suitCode = card.slice(-1);
  const rawRank = card.slice(0, -1);
  const rank = rawRank === "T" ? "10" : rawRank;
  return `${rank}${{ h: "♥", d: "♦", c: "♣", s: "♠" }[suitCode] || suitCode}`;
}

function oddsPercent(permille) {
  if (permille == null) return "—";
  return `${(permille / 10).toFixed(permille % 10 === 0 ? 0 : 1)}%`;
}

function ShowdownOdds({ odds, seats, leaders }) {
  if (!odds?.length) return null;
  return html`<div class="showdown-odds" aria-label="Showdown odds">
    ${odds.map((entry) => {
      const seat = seats.find((candidate) => candidate.index === entry.seat);
      const name = seat?.display_name || seat?.occupant || `Seat ${entry.seat}`;
      const classes = leaders.includes(entry.seat) ? "leading" : "";
      return html`<span class=${classes}><b>${name}</b><strong>${oddsPercent(entry.equity_permille)}</strong><small>${entry.outs?.length ? entry.outs.map(cardText).join(" ") : "\u00A0"}</small></span>`;
    })}
  </div>`;
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

function TableLog({ events, seats, summary, settled, status }) {
  const results = settled ? winnerLines(summary, seats) : [];
  // Awards are the punchline; they wait for the last card like everything else.
  const shown = settled ? events : events.filter((event) => event.kind !== "Award");
  return html`<section class="game-log" aria-live="polite"><ol>${status && html`<li class="status-log"><span>${status.street}</span><b>${status.label}</b></li>`}${results.map((result) => html`<li class="result-log"><span>Result</span><b>${result}</b></li>`)}${shown.slice(-16).toReversed().map((event) => html`<li><span>${streetName(event.street)}</span><b>${eventLabel(event, seats)}</b></li>`)}</ol></section>`;
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

function tournamentChampion(state) {
  if (!state.tournament?.finished) return null;
  const eliminated = new Set(state.tournament.finish_order || []);
  return state.seats
    .filter((seat) => seat.occupant !== "empty" && !eliminated.has(seat.index))
    .toSorted((left, right) => right.stack - left.stack)[0]
    || state.seats.find((seat) => seat.index === state.tournament.finish_order?.at(-1))
    || null;
}

function TournamentComplete({ champion }) {
  const name = champion?.display_name || champion?.occupant || "Winner";
  return html`<section class="tournament-complete" aria-live="polite">
    <small>Tournament complete</small>
    <b>${name} wins</b>
  </section>`;
}

function TableCommand({ label, endpoint, href, disabled, forfeits, buyIn, refresh }) {
  if (href) return html`<a class="table-command table-command-link" href=${href}>${label}</a>`;
  const submit = async () => {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    if (response.ok) {
      refresh();
      refreshBank().catch(() => {});
    } else document.getElementById("table-error").textContent = await responseError(response);
  };
  // Walking out of a tournament is not a cash-out: the entry is gone, so ask first.
  if (forfeits) {
    return html`<span class="table-command-confirm">
      <button class="table-command" type="button" commandfor="forfeit-entry" command="show-modal">${label}</button>
      <dialog id="forfeit-entry" class="confirm-dialog">
        <form method="dialog">
          <header><h2>Leave the tournament?</h2></header>
          <p>You forfeit your entry. The ${money(buyIn)} buy-in stays in the prize pool, your chips leave the table, and you finish in your current place.</p>
          <footer>
            <button type="submit" value="stay">Keep playing</button>
            <button class="danger" type="button" commandfor="forfeit-entry" command="close" onClick=${submit}>Forfeit and leave</button>
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
  if (state.tournament?.finished) {
    return html`<${TableCommand} label="Leave" href="/tables" />`;
  }
  // A table full of house players still has room: you take one of their seats.
  const seatsForYou = state.tournament
    ? openSeats
    : [...openSeats, ...state.seats.filter((seat) => seat.bot)];
  const canAffordCashSeat = state.tournament || (state.bank_balance ?? 0) >= state.buy_in;
  const viewer = state.viewer_seat == null
    ? null
    : state.seats.find((seat) => seat.index === state.viewer_seat);
  const eliminatedFromTournament = Boolean(state.tournament && state.viewer_eliminated);
  const leave = {
    label: "Leave",
    endpoint: `/tables/${tableId}/leave`,
    forfeits: Boolean(state.tournament && !eliminatedFromTournament),
  };
  const commands = [];
  if (state.viewer_leaving) {
    commands.push({ label: "Leaving...", disabled: true });
  } else if (viewer || state.viewer_eliminated) {
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
  useEffect(() => {
    const syncBalance = (event) => {
      if (!event.detail) return;
      setState((current) => current && { ...current, bank_balance: event.detail.balance });
    };
    window.addEventListener("bank:updated", syncBalance);
    return () => window.removeEventListener("bank:updated", syncBalance);
  }, []);
  useHeaderInfo(state?.tournament);
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
  const tournamentComplete = Boolean(state.tournament?.finished && (!showdown || settled));
  const champion = tournamentComplete ? tournamentChampion(state) : null;
  const status = showdown
    ? null
    : hand
      ? { street: streetName(hand.street), label: `${currentName} to act${hand.to_call ? ` · ${money(hand.to_call)} to call` : ""}` }
      : { street: "Table", label: state.can_deal ? "Nobody seated · deal a hand" : "Waiting for players" };
  const renderSeat = (seat) => html`<${Seat} seat=${seat} player=${hand?.players?.find((player) => player.seat === seat.index)} events=${hand?.events || showdown?.events || []} current=${hand?.current_player === seat.index} viewer=${seat.index === state.viewer_seat} viewerCards=${hand?.your_hole_cards || []} button=${state.button} showdown=${showdown} leading=${runout.leaders.includes(seat.index)} settled=${settled} champion=${champion?.index === seat.index} />`;
  return html`<div class=${`table-shell ${settings.paranoid ? "paranoid-cards" : ""}`}>
    <${CardSettings} settings=${settings} setSettings=${setSettings} interactive=${true} concealable=${true} trigger=${false} />
    <section class="table-stage" aria-label="Poker table">
      <div class="seats other-seats" data-seat-total=${otherSeats.length}>${otherSeats.map(renderSeat)}</div>
      <div class="felt">
        <div class="table-center">
          ${(hand || showdown) && html`<div class="table-metrics"><span><small>Pot</small><b>${money(hand?.pot || showdown?.awards?.reduce((sum, award) => sum + award.amount, 0) || 0)}</b></span><span class=${hand ? "" : "metric-placeholder"}><small>Current bet</small><b>${hand ? money(hand.last_bet) : "\u00A0"}</b></span></div>`}
          <div class="board">${board.map((card) => html`<${Card} card=${card} interactive=${true} />`)}</div>
          ${(hand || showdown) && html`<div class="table-rail">
            ${showdown && !settled && html`<${ShowdownOdds} odds=${runout.odds} seats=${state.seats} leaders=${runout.leaders} />`}
            <p class="showdown-result">${showdown ? result : ""}</p>
          </div>`}
        </div>
      </div>
      ${viewerSeat && html`<div class="seats viewer-seats" data-seat-total="1">${renderSeat(viewerSeat)}</div>`}
    </section>
    <section class="decision-area">${tournamentComplete
      ? html`<${TournamentComplete} champion=${champion} />`
      : showdown && !awaitingDeal
      ? html`<${ShowdownAdvance} remaining=${remaining} duration=${resultPause} canContinue=${settled && state.viewer_seat != null} refresh=${refresh} />`
      : hand?.legal_actions
        ? html`<${Actions} hand=${hand} seats=${state.seats} tableId=${tableId} settings=${settings} refresh=${refresh} />`
        : state.can_deal
          ? html`<${DealHouseHand} refresh=${refresh} />`
          : null}</section>
    <${TableLog} events=${handEvents} seats=${state.seats} summary=${showdown} settled=${settled} status=${status} />
    <nav class="table-controls"><p id="table-error" class="error" role="alert"></p><a class="table-history-link" href=${`/tables/${tableId}/history`}>History</a><${SeatBot} state=${state} openSeats=${openSeats} refresh=${refresh} /><${TableCommands} state=${state} openSeats=${openSeats} refresh=${refresh} /></nav>
  </div>`;
}

if (root) render(html`<${TableApp} />`, root);
