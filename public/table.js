import { html, render, useEffect, useRef, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings, useCardSettings } from "/public/card-settings.js";
import { refreshBank, responseError, useOverflowTitle, usePending, useResultClock, wholeDollarMoney as money } from "/public/shared.js";
// Card geometry contracts live in card.js: rawRank === "T" ? "10", card-corner rank over suit.

const root = document.getElementById("table-app");
const tableId = root?.dataset.tableId;
const SHOWDOWN_PAUSE_MS = 6_000;
const FOLD_RESULT_PAUSE_MS = 3_000;
// Matches RUNOUT_STEP_SECONDS: how long a card sits before it turns itself.
const RUNOUT_STEP_MS = 5_000;
// Matches RUNOUT_FLOOR_MS: a card keeps the table's attention this long before
// anyone may skip past it, so the button holds rather than refusing a press.
const RUNOUT_FLOOR_MS = 1_200;
// The last stretch of somebody's turn, when the bar turns urgent.
const URGENT_TURN_MS = 3_000;

// An all-in board runs out on the server, a street per advance, while the hand
// is still live (SPEC V59). The client renders what is on the table; it no
// longer replays a decided result against a clock.
function runoutState(hand) {
  // A parked runout always has cards still to come -- the river resolves the
  // hand rather than being held -- so somebody is genuinely ahead.
  if (hand?.awaiting_advance) {
    return { leaders: hand.runout_leaders || [], odds: hand.runout_odds || [], live: true };
  }
  // A settled hand has a winner, not a leader, so AHEAD stands down (SPEC V59).
  return { leaders: [], odds: [], live: false };
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

// How long the player to act has left. The whole table watches it -- knowing
// somebody is nearly out of time is half of why the clock is there -- so it
// draws on the acting seat as well as under the viewer's own buttons.
function TurnClock({ remaining, duration, className, announce }) {
  const left = duration > 0 ? Math.max(0, Math.min(1, remaining / duration)) : 0;
  const seconds = Math.ceil(remaining / 1000);
  return html`<span
    class=${`turn-clock ${className} ${remaining <= URGENT_TURN_MS ? "urgent" : ""}`}
    role=${announce ? "timer" : undefined}
    aria-label=${announce ? `${seconds}s to act` : undefined}
    aria-hidden=${announce ? undefined : "true"}
  ><i style=${{ width: `${left * 100}%` }}></i></span>`;
}

function Seat({ seat, player, events, street, current, button, viewer, viewerCards, showdown, revealed, leading, settled, champion, clock }) {
  const label = seat.display_name || seat.occupant;
  const role = blindRole(events, seat.index);
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
  // A person's name is a way through to their page (and to handing them
  // money); the house has no page to go to.
  const name = seat.user_id
    ? html`<a class="player-link" href=${`/player/${seat.user_id}`}>${label}</a>`
    : label;
  const playerInfo = html`<span class="player-info" tabindex="0">
    <strong>${name}</strong><i aria-hidden="true">ⓘ</i>
    <span class="player-tooltip" role="tooltip"><b>Lifetime balance ${seat.bank_balance == null ? "Unavailable" : money(seat.bank_balance)}</b><span>Stack ${money(stack)}</span>${seat.bank_entries.slice(-3).toReversed().map((entry) => html`<small>${entry.memo}: ${entry.delta >= 0 ? "+" : ""}${money(entry.delta)}</small>`)}</span>
  </span>`;
  const stackLabel = html`<span class="seat-stack">${money(stack)}</span>`;
  const checked = Boolean(player && street && events.some((event) => event.seat === seat.index && event.street === street && event.kind === "Check"));
  const wager = html`<span class=${`seat-wager ${player?.street_contribution > 0 || player?.all_in || checked ? "" : "no-wager"} ${checked ? "checked-wager" : ""}`}>${player?.all_in ? "ALL IN" : checked ? "CHECKED" : money(player?.street_contribution || 0)}</span>`;
  const positionBadges = html`<span class="seat-corner-badges">${seat.index === button && html`<i class="seat-role button-role">D</i>`}${role && html`<i class="seat-role">${role}</i>`}</span>`;
  return html`<article class=${classes}>
    ${viewer
      ? html`${positionBadges}<span class="viewer-summary">${playerInfo}${stackLabel}${wager}</span>`
      : html`<span class="opponent-heading">${playerInfo}${positionBadges}</span>${stackLabel}`}
    ${cards.length > 0
      ? html`<span class=${`seat-cards ${revealed ? "revealed" : viewer ? "owned" : "hidden"}`}>${cards.map((card) => html`<${Card} card=${card} hidden=${card == null} interactive=${viewer} />`)}</span>`
      // A seat between hands still holds the space its cards had, or the whole
      // table shrinks the moment a hand ends and the controls below slide up
      // under whatever finger was on its way to them. Your own panel lays its
      // hand out sideways, so it has to hold both cards' width, not one card's:
      // a single slot would let the panel narrow and slide the hand across the
      // moment a hand ended.
      : html`<span class="seat-cards vacant">${Array.from({ length: viewer ? 2 : 1 }, () => html`<span class="playing-card slot-card" aria-hidden="true"></span>`)}${player?.folded && !viewer && html`<span class="seat-card-state"><i class="seat-role state-role">FOLDED</i></span>`}</span>`}
    ${!viewer && wager}
    <span class="seat-outcome-badges">${leading && html`<i class="seat-role leading-role">AHEAD</i>`}${winner && html`<i class="seat-role winner-role">WINNER</i>`}</span>
    ${clock && html`<${TurnClock} ...${clock} className="seat-clock" announce=${viewer} />`}
  </article>`;
}

// The compact layout, matching the breakpoint 05-table.css switches on. The
// action bar's middle column carries Check or Call beside the presets, so a
// phone has room for two of them where a desktop has room for three.
const COMPACT_LAYOUT = "(max-width:640px),(max-height:520px)";

function useCompactLayout() {
  const query = useRef(null);
  query.current ||= window.matchMedia(COMPACT_LAYOUT);
  const [compact, setCompact] = useState(query.current.matches);
  useEffect(() => {
    const media = query.current;
    const sync = () => setCompact(media.matches);
    media.addEventListener("change", sync);
    sync();
    return () => media.removeEventListener("change", sync);
  }, []);
  return compact;
}

// Preset wagers are named by the street total they raise to. The ladder is
// priority-ordered -- the sizes worth a button when only two fit come first --
// and the situation picks it: preflop opens and three-bets are sized in blinds
// and in multiples of the outstanding bet, because pot fractions preflop all
// land under the minimum and collapse onto it. Postflop sizes are fractions of
// the pot the raise would leave behind, so calling first is part of the price.
function wagerLadder(hand, blind, toCall, contribution) {
  const preflop = hand.street === "Preflop";
  const potAfterCall = hand.pot + toCall;
  const raiseTo = (candidates) => candidates.map(([total, reason]) => ({ amount: total - contribution, reason }));
  if (preflop && hand.last_bet <= blind) {
    // An unopened pot is opened in blinds, plus one for every limper to talk
    // through.
    const limpers = hand.players.filter((player) => player.street_contribution === blind).length - 1;
    const open = (multiple) => (multiple + Math.max(0, limpers)) * blind;
    return raiseTo([[open(3), "3x"], [open(4), "4x"], [open(5), "5x"]]);
  }
  if (preflop) {
    return raiseTo([[3 * hand.last_bet, "3x"], [2.5 * hand.last_bet, "2.5x"], [4 * hand.last_bet, "4x"]]);
  }
  if (toCall === 0) {
    return [[0.5, "Half pot"], [1, "Pot"], [1 / 3, "Third pot"], [0.75, "Three-quarter pot"]]
      .map(([fraction, reason]) => ({ amount: fraction * hand.pot, reason }));
  }
  return [[0.5, "Half pot"], [1, "Pot"], [0.75, "Three-quarter pot"]]
    .map(([fraction, reason]) => ({ amount: toCall + fraction * potAfterCall, reason }));
}

function wagerOptions(hand, limit) {
  const wager = hand?.legal_actions?.wager;
  if (!wager) return [];
  const player = hand.players.find((candidate) => candidate.seat === hand.legal_actions.seat);
  const contribution = player?.street_contribution || 0;
  const toCall = hand.legal_actions.to_call || 0;
  const blind = hand.big_blind || 1;
  // A fixed-limit street offers the one legal size and nothing to choose from.
  const candidates = wager.fixed != null
    ? [{ amount: wager.fixed, reason: "Fixed wager" }]
    // The minimum trails the ladder rather than leading it: it earns a button
    // only when no real size survives, so a short stack still has one to press.
    : [...wagerLadder(hand, blind, toCall, contribution), { amount: wager.min, reason: "Minimum" }];
  const kept = [];
  for (const candidate of candidates) {
    if (kept.length >= limit) break;
    const rounded = Math.round(candidate.amount / blind) * blind;
    const amount = Math.max(wager.min, Math.min(wager.max, rounded));
    if (amount <= 0) continue;
    // All In already sits at the end of the bar, so a preset that all but
    // shoves is a second button for the same decision.
    if (wager.fixed == null && wager.max - amount < Math.max(blind, wager.max * 0.1)) continue;
    // Two sizes a blind apart -- or within 15% of each other -- are one choice
    // wearing two buttons. Exact-match dedupe let those twins through.
    const twin = kept.some((other) => Math.abs(other.amount - amount) < Math.max(blind, 0.15 * Math.min(other.amount, amount)));
    if (twin) continue;
    // `amount` is the chips this action adds; the button shows the street
    // total it raises to, so it never reads the same as the call beside it.
    kept.push({ amount, total: contribution + amount, reason: candidate.reason });
  }
  return kept.toSorted((left, right) => left.amount - right.amount);
}

function HoldAction({ label, class: className, disabled, hold, submit, ariaLabel, ...rest }) {
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
    if (!hold || disabled || timer.current != null) return;
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
    ...${rest}
    class=${`${className} ${hold ? "hold-action" : ""} ${holding ? "holding" : ""}`}
    type="button"
    disabled=${disabled}
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

// The preset wagers cover the common lines; this opens the whole legal range
// for the spots they miss. The slider works in whole dollars, with both ends
// of the range reachable so a shove-sized raise is always one drag away.
function CustomWager({ label, wager, contribution, disabled, className, submit }) {
  const dialog = useRef(null);
  const dollars = 100;
  const steps = Math.max(1, Math.ceil((wager.max - wager.min) / dollars));
  const amountAt = (index) => (index >= steps ? wager.max : Math.min(wager.max, wager.min + index * dollars));
  const [open, setOpen] = useState(false);
  const [index, setIndex] = useState(0);
  // A new decision brings new bounds; the old pick rarely survives them.
  useEffect(() => setIndex(0), [wager.min, wager.max]);
  // The picker only exists while it is up, so a closed dialog leaves no stray
  // buttons behind the action bar.
  useEffect(() => {
    if (open && dialog.current && !dialog.current.open) dialog.current.showModal();
  }, [open]);
  const amount = amountAt(index);
  const total = money(contribution + amount);
  const choose = () => {
    dialog.current?.close();
    submit(amount);
  };
  return html`<span class="custom-wager">
    <button class=${className} type="button" disabled=${disabled} aria-label=${`${label} a custom amount`} onClick=${() => { setIndex(0); setOpen(true); }}><span>${label}…</span></button>
    ${open && html`<dialog class="wager-dialog" ref=${dialog} onClose=${() => setOpen(false)}>
      <form method="dialog">
        <header><h2>${label} to</h2><output>${total}</output></header>
        <input
          type="range"
          min="0"
          max=${steps}
          step="1"
          value=${index}
          aria-label=${`${label} amount`}
          aria-valuetext=${total}
          onInput=${(event) => setIndex(Number(event.target.value))}
        />
        <p class="wager-range"><span>${money(contribution + wager.min)}</span><span>${money(contribution + wager.max)}</span></p>
        <footer>
          <button type="submit" value="cancel">Cancel</button>
          <button class="wager-confirm" type="button" onClick=${choose}>${total}</button>
        </footer>
      </form>
    </dialog>`}
  </span>`;
}

function Actions({ hand, seats, tableId: actionTableId, settings, refresh }) {
  const actions = new Set((hand?.legal_actions?.actions || []).map(actionName));
  const [pending, run] = usePending();
  const submit = (kind, amount) => run(`${kind}:${amount ?? ""}`, async () => {
    const response = await fetch(`/tables/${actionTableId}/action`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ kind, amount }) });
    // Hold the button until the table has actually moved on, not merely until
    // the server said yes.
    if (response.ok) await refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  });
  const busy = (key, className) => ({
    class: `${className}${pending === key ? " pending" : ""}`,
    disabled: pending != null,
    "aria-busy": pending === key,
  });
  const compact = useCompactLayout();
  const wagerKind = actions.has("Bet") ? "bet" : "raise";
  const wagerLabel = wagerKind === "bet" ? "Bet" : "Raise";
  const actor = seats.find((seat) => seat.index === hand.legal_actions.seat);
  const callIsAllIn = Boolean(actor && actions.has("Call") && (hand.legal_actions.to_call || 0) >= actor.stack);
  // A shove by a shorter stack caps the pot: the call is the last decision of
  // the hand, so it takes the All In slot and its colour while keeping its own
  // label -- the caller still has chips behind and is not going all in.
  const cappedCall = Boolean(
    actions.has("Call") && !callIsAllIn && !actions.has("Bet") && !actions.has("Raise") && !actions.has("AllIn"),
  );
  const wagers = wagerOptions(hand, compact ? 2 : 3);
  const wagerBounds = hand.legal_actions.wager;
  // Only worth opening when there is a range to pick from: a fixed limit wager
  // or a stack with exactly one legal raise left has nothing to slide over.
  const showCustomWager = Boolean(
    (actions.has("Bet") || actions.has("Raise")) && wagerBounds && wagerBounds.fixed == null && wagerBounds.max > wagerBounds.min,
  );
  const actorContribution = hand.players.find((candidate) => candidate.seat === hand.legal_actions.seat)?.street_contribution || 0;
  const middleCount = Number(actions.has("Check")) + Number(actions.has("Call") && !callIsAllIn && !cappedCall) + wagers.length;
  const showAllIn = actions.has("AllIn") || callIsAllIn;
  return html`<div class="actions" aria-label="Actions">
    <span class="action-edge action-edge-left">${actions.has("Fold") && html`<${HoldAction} label="Fold" hold=${settings.confirmFold} submit=${() => submit("fold")} ...${busy("fold:", "danger fold-action")} />`}</span>
    <span class="action-middle" style=${`--middle-action-count:${Math.max(1, middleCount)}`}>
      ${actions.has("Check") && html`<button ...${busy("check:", "primary-action")} onClick=${() => submit("check")}><span>Check</span></button>`}
      ${actions.has("Call") && !callIsAllIn && !cappedCall && html`<button ...${busy("call:", "primary-action")} aria-label=${`Call ${money(hand.legal_actions.to_call)}`} onClick=${() => submit("call")}><span class="action-prefix">Call </span><span class="action-amount">${money(hand.legal_actions.to_call)}</span></button>`}
      ${(actions.has("Bet") || actions.has("Raise")) && wagers.map((option) => html`<button ...${busy(`${wagerKind}:${option.amount}`, "wager-action")} aria-label=${`${wagerLabel} to ${money(option.total)}`} title=${`${wagerLabel} to ${money(option.total)} · ${option.reason}`} onClick=${() => submit(wagerKind, option.amount)}><span class="action-amount">${money(option.total)}</span></button>`)}
    </span>
    <span class="action-edge action-edge-right">${showCustomWager && html`<${CustomWager}
      label=${wagerLabel}
      wager=${wagerBounds}
      contribution=${actorContribution}
      disabled=${pending != null}
      className=${`wager-action custom-wager-action${pending?.startsWith(`${wagerKind}:`) ? " pending" : ""}`}
      submit=${(amount) => submit(wagerKind, amount)}
    />`}${cappedCall
      ? html`<button ...${busy("call:", "wager-action all-in-action capped-call")} aria-label=${`Call ${money(hand.legal_actions.to_call)}`} onClick=${() => submit("call")}><span class="action-prefix">Call </span><span class="action-amount">${money(hand.legal_actions.to_call)}</span></button>`
      : showAllIn && html`<${HoldAction} label="All In" hold=${settings.confirmAllIn} submit=${() => submit(callIsAllIn ? "call" : "all_in")} ...${busy(callIsAllIn ? "call:" : "all_in:", "wager-action all-in-action")} />`}</span>
  </div>`;
}

// With nobody sitting down, the house waits to be asked for a hand.
function DealHouseHand({ refresh }) {
  const [pending, run] = usePending();
  const deal = () => run("deal", async () => {
    const response = await fetch(`/tables/${tableId}/deal`, { method: "POST" });
    if (response.ok) await refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  });
  const busy = pending != null;
  return html`<div class="showdown-advance house-deal"><button class=${busy ? "pending" : ""} type="button" disabled=${busy} aria-busy=${busy} onClick=${deal}><b>Deal a hand</b></button></div>`;
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

function ShowdownAdvance({ remaining, duration, canContinue, refresh }) {
  const [pending, run] = usePending();
  const seconds = Math.ceil(remaining / 1000);
  const width = `${(remaining / duration) * 100}%`;
  const label = `Next hand in ${seconds}s`;
  // A board still running out is not skippable, so offer no button to press.
  if (!canContinue) return html`<div class="showdown-advance spectator"><span class="showdown-progress" style=${{ width }}></span><b>${label}</b></div>`;
  const advance = () => run("continue", async () => {
    const response = await fetch(`/tables/${tableId}/continue`, { method: "POST" });
    if (response.ok) await refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  });
  const busy = pending != null;
  return html`<div class="showdown-advance"><button class=${busy ? "pending" : ""} type="button" disabled=${busy} aria-busy=${busy} aria-label=${`Continue now. ${label}`} onClick=${advance}><span class="showdown-progress" style=${{ width }}></span><b>OK · ${seconds}s</b></button></div>`;
}

// The next card turns itself on the server's deadline, so it counts down the
// same way the next hand does; pressing only brings it forward (SPEC V59).
function RunoutAdvance({ remaining, duration, floorMs, seated, refresh }) {
  const label = "Next card";
  const [pending, run] = usePending();
  const seconds = Math.ceil(remaining / 1000);
  const width = `${(remaining / duration) * 100}%`;
  const countdown = Number.isFinite(remaining) ? ` · ${seconds}s` : "";
  // The card has just landed; hold the button rather than let a press bounce.
  // A lone human has nobody else to hold it open for, so the floor is 0.
  const held = remaining > duration - floorMs;
  if (!seated) return html`<div class="showdown-advance spectator"><span class="showdown-progress" style=${{ width }}></span><b>${label}${countdown}</b></div>`;
  const advance = () => run("advance", async () => {
    const response = await fetch(`/tables/${tableId}/advance`, { method: "POST" });
    if (response.ok) await refresh();
    else document.getElementById("table-error").textContent = await responseError(response);
  });
  const busy = pending != null;
  return html`<div class="showdown-advance"><button class=${busy ? "pending" : ""} type="button" disabled=${busy || held} aria-busy=${busy} aria-label=${`${label} now. ${label}${countdown}`} onClick=${advance}><span class="showdown-progress" style=${{ width }}></span><b>${label}${countdown}</b></button></div>`;
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
  const labelRef = useOverflowTitle(label);
  const [pending, run] = usePending();
  const submit = () => run("submit", async () => {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    if (response.ok) {
      refreshBank().catch(() => {});
      await refresh();
    } else document.getElementById("table-error").textContent = await responseError(response);
  });
  const busy = pending != null;
  if (href) return html`<a class="table-command table-command-link" ref=${labelRef} href=${href}>${label}</a>`;
  // Walking out of a tournament is not a cash-out: the entry is gone, so ask first.
  if (forfeits) {
    return html`<span class="table-command-confirm">
      <button class=${`table-command ${busy ? "pending" : ""}`} type="button" ref=${labelRef} disabled=${busy} aria-busy=${busy} commandfor="forfeit-entry" command="show-modal">${label}</button>
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
  return html`<button class=${`table-command ${busy ? "pending" : ""}`} type="button" ref=${labelRef} disabled=${disabled || busy} aria-busy=${busy} onClick=${submit}>${label}</button>`;
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
    : [...openSeats, ...state.seats.filter((seat) => seat.bot && !seat.reserved)];
  // A cheap table covers the shortfall with a loan, so the seat is offered
  // whatever the balance is; a deeper one you pay for yourself.
  const canAffordCashSeat = state.lends_buy_in || (state.bank_balance ?? 0) >= state.buy_in;
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
  } else if (state.viewer_joining) {
    // The seat is paid for; the hand already running has to end first.
    commands.push({ label: "Seated next hand...", disabled: true });
    commands.push({ ...leave, label: "Cancel" });
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

// The stakes decide who the house will sit: no fish from $100,000 up, sharks
// only from $500,000 up. The server enforces it; this keeps the offer honest.
const NO_FISH_FROM = 10_000_000;
const SHARKS_ONLY_FROM = 50_000_000;

function botKindsFor(buyIn) {
  if (buyIn >= SHARKS_ONLY_FROM) return ["shark"];
  if (buyIn >= NO_FISH_FROM) return ["rock", "grinder", "shark"];
  return ["fish", "rock", "grinder", "shark"];
}

function SeatBot({ state, openSeats, refresh }) {
  if (openSeats.length === 0 || state.tournament?.started) return null;
  const [pending, run] = usePending();
  const submit = (kind) => run(kind, async () => {
    const response = await fetch(`/tables/${tableId}/bot`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ kind }) });
    if (response.ok) {
      await refresh();
    } else document.getElementById("table-error").textContent = await responseError(response);
  });
  return html`<span class="seat-bot" aria-label="Seat a bot">
    ${botKindsFor(state.buy_in).map((kind) => html`<button class=${pending === kind ? "pending" : ""} type="button" disabled=${pending != null} aria-busy=${pending === kind} onClick=${() => submit(kind)}>Seat ${kind}</button>`)}
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
  const advanceRemaining = useResultClock(Boolean(state?.hand?.awaiting_advance), state?.advance_at, RUNOUT_STEP_MS);
  // A table with more than one person at it puts whoever is to act on a clock;
  // when it runs out the server checks or folds for them.
  const turnDuration = 1000 * (state?.turn_seconds || 10);
  const turnRemaining = useResultClock(Boolean(state?.turn_deadline), state?.turn_deadline, turnDuration);
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
  const runout = runoutState(hand);
  // Hole cards are face up for the rest of the hand the moment betting closes,
  // and stay up in the settled summary (SPEC V59).
  // The server is the only thing that decides whose cards may be seen (SPEC
  // V3), so any hole cards in the state are meant for this viewer: the runout,
  // the settled summary, or the bot x-ray during ordinary betting. Once betting
  // closes every unfolded seat is exposed, the viewer's own included (V59);
  // before that the viewer's row keeps drawing from `your_hole_cards`, so it
  // stays subject to paranoid mode.
  const exposed = Boolean(hand?.awaiting_advance || showdown);
  const revealedBySeat = new Map([
    ...(showdown?.revealed_hole_cards || []),
    ...(hand?.seats || [])
      .filter((entry) => entry.hole_cards?.length && (exposed || entry.index !== state.viewer_seat))
      .map((entry) => [entry.index, entry.hole_cards]),
  ]);
  const board = hand?.board || showdown?.board || [];
  const openSeats = state.seats.filter((seat) => seat.occupant === "empty" && !seat.reserved);
  // Nothing is settled until the hand has left the table, and it cannot leave
  // before its last card is face up (SPEC V59).
  const settled = !hand;
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
  const turnClock = state.turn_deadline ? { remaining: turnRemaining, duration: turnDuration } : null;
  const renderSeat = (seat) => html`<${Seat} seat=${seat} player=${hand?.players?.find((player) => player.seat === seat.index)} events=${hand?.events || showdown?.events || []} street=${hand?.street} current=${hand?.current_player === seat.index} viewer=${seat.index === state.viewer_seat} viewerCards=${hand?.your_hole_cards || []} button=${state.button} showdown=${showdown} revealed=${revealedBySeat.get(seat.index)} leading=${runout.leaders.includes(seat.index)} settled=${settled} champion=${champion?.index === seat.index} clock=${hand?.current_player === seat.index ? turnClock : null} />`;
  return html`<div class=${`table-shell ${settings.paranoid ? "paranoid-cards" : ""}`}>
    <section class="table-stage" aria-label="Poker table">
      <div class="seats other-seats" data-seat-total=${otherSeats.length}>${otherSeats.map(renderSeat)}</div>
      <div class="felt">
        <div class="table-center">
          ${/* The metrics keep their box between hands rather than leaving with
                the hand: dropping them shortened the felt, and everything below
                it — your own hand most of all — jumped up the screen. */ ""}
          <div class="table-metrics"><span class=${hand || showdown ? "" : "metric-placeholder"}><small>Pot</small><b>${hand || showdown ? money(hand?.pot || showdown?.awards?.reduce((sum, award) => sum + award.amount, 0) || 0) : "\u00A0"}</b></span><span class=${hand ? "" : "metric-placeholder"}><small>Current bet</small><b>${hand ? money(hand.last_bet) : "\u00A0"}</b></span></div>
          ${/* An empty board still holds a card's height, so the felt does not
                shorten between hands and lift everything under it. */ ""}
          <div class="board">${board.length > 0
            ? board.map((card) => html`<${Card} card=${card} interactive=${true} />`)
            : html`<span class="playing-card slot-card" aria-hidden="true"></span>`}</div>
          <div class="table-rail">
            ${runout.live && html`<${ShowdownOdds} odds=${runout.odds} seats=${state.seats} leaders=${runout.leaders} />`}
            <p class="showdown-result">${showdown ? result : ""}</p>
          </div>
        </div>
      </div>
      ${viewerSeat && html`<div class="seats viewer-seats" data-seat-total="1">${renderSeat(viewerSeat)}</div>`}
      <${CardSettings} settings=${settings} setSettings=${setSettings} interactive=${true} concealable=${true} trigger=${false} />
    </section>
    <section class="decision-area">${tournamentComplete
      ? html`<${TournamentComplete} champion=${champion} />`
      : hand?.awaiting_advance
      ? html`<${RunoutAdvance} remaining=${advanceRemaining} duration=${RUNOUT_STEP_MS} floorMs=${state.runout_floor_ms ?? RUNOUT_FLOOR_MS} seated=${state.viewer_seat != null} refresh=${refresh} />`
      : showdown && !awaitingDeal
      ? html`<${ShowdownAdvance} remaining=${remaining} duration=${resultPause} canContinue=${settled && state.viewer_seat != null} refresh=${refresh} />`
      : hand?.legal_actions
        ? html`<${Actions} hand=${hand} seats=${state.seats} tableId=${tableId} settings=${settings} refresh=${refresh} />`
        : state.can_deal
          ? html`<${DealHouseHand} refresh=${refresh} />`
          : null}</section>
    <aside class="table-side-rail">
      <${TableLog} events=${handEvents} seats=${state.seats} summary=${showdown} settled=${settled} status=${status} />
      <nav class="table-controls"><p id="table-error" class="error" role="alert"></p><a class="table-history-link" href=${`/tables/${tableId}/history`}>History</a><${SeatBot} state=${state} openSeats=${openSeats} refresh=${refresh} /><${TableCommands} state=${state} openSeats=${openSeats} refresh=${refresh} /></nav>
    </aside>
  </div>`;
}

if (root) render(html`<${TableApp} />`, root);
