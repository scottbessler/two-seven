import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings } from "/public/card-settings.js";
import { money, refreshBank, responseError, usePending, useResultClock, wholeDollarMoney } from "/public/shared.js";

const root = document.getElementById("blackjack-app");
const tableId = root?.dataset.tableId;
const TRAINER_KEYS = {
  counting_tutor: "blackjack-counting-tutor",
  counting_quiz: "blackjack-counting-quiz",
  bet_analyzer: "blackjack-bet-analyzer",
};

function readTrainerSettings() {
  return Object.fromEntries(Object.entries(TRAINER_KEYS).map(([name, key]) => [name, localStorage.getItem(key) === "on"]));
}

function DealerHand({ cards, score, hidden }) {
  // A hand keeps drawing until it stands or busts, so the card count is not a
  // constant the stylesheet can assume. `--card-count` hands it to CSS, which
  // divides the hand's width by it and keeps a long hand on screen.
  const count = cards.length + (hidden ? 1 : 0);
  return html`<section class="blackjack-hand blackjack-dealer-hand" aria-label=${`Dealer${score == null ? "" : `, ${score}`}`}>
    <div class="board" style=${`--card-count:${count}`}>
      ${cards.map((card) => html`<${Card} value=${card} interactive=${true} />`)}
      ${hidden ? html`<${Card} hidden=${true} interactive=${true} />` : null}
      ${score == null ? null : html`<strong class="blackjack-hand-score">${score}</strong>`}
    </div>
  </section>`;
}

function PlayerHand({ hand, index, count, active, bet, deadline, duration }) {
  const name = count > 1 ? `Hand ${index + 1}` : "You";
  return html`<section class=${`blackjack-player-hand${active ? " active" : ""}`} aria-label=${`${name}, ${hand.score}${active ? ", your turn" : ""}`}>
    <div class="board" style=${`--card-count:${hand.cards.length}`}>
      ${hand.cards.map((card) => html`<${Card} value=${card} interactive=${true} />`)}
    </div>
    <div class="blackjack-player-summary">
      <span>${name}</span>
      <strong>${hand.score}</strong>
      ${bet == null ? null : html`<small>Bet ${money(bet)}</small>`}
    </div>
    ${active && deadline ? html`<${TurnClock} deadline=${deadline} duration=${duration} />` : null}
  </section>`;
}

function TurnClock({ deadline, duration }) {
  const remaining = useResultClock(Boolean(deadline), deadline, duration);
  return html`<span class=${`turn-clock blackjack-turn-clock${remaining < duration / 4 ? " urgent" : ""}`} role="timer" aria-label="Turn clock">
    <i style=${`width:${(100 * remaining) / duration}%`}></i>
  </span>`;
}

function TrainerSettings({ settings, setSettings, onChange }) {
  const toggle = (name) => (event) => {
    const next = { ...settings, [name]: event.currentTarget.checked };
    localStorage.setItem(TRAINER_KEYS[name], next[name] ? "on" : "off");
    setSettings(next);
    onChange(next);
  };
  return html`
    <label class="card-option-toggle"><input name="counting-tutor" type="checkbox" checked=${settings.counting_tutor} onChange=${toggle("counting_tutor")} /><span><b>Card counting tutor</b><small>Show the Hi-Lo running count and card-by-card changes</small></span></label>
    <label class="card-option-toggle"><input name="counting-quiz" type="checkbox" checked=${settings.counting_quiz} onChange=${toggle("counting_quiz")} /><span><b>Card counting quiz</b><small>Ask for the running count after each round</small></span></label>
    <label class="card-option-toggle"><input name="bet-analyzer" type="checkbox" checked=${settings.bet_analyzer} onChange=${toggle("bet_analyzer")} /><span><b>Bet analyzer</b><small>Compare your choices with basic strategy</small></span></label>
  `;
}

function TrainerPanel({ trainer, quizChoice, setQuizChoice }) {
  if (!trainer) return null;
  return html`
    ${trainer.count ? html`<section class="blackjack-trainer-count" aria-label="Card counting tutor">
      <span><b>${trainer.count.running}</b> running</span>
      <span><b>${trainer.count.true_count.toFixed(1)}</b> true</span>
      <span><b>${trainer.count.penetration_percent}%</b> seen</span>
    </section>` : null}
    ${trainer.log?.length ? html`<ol class="blackjack-trainer-log" aria-label="Count log">${trainer.log.map((line) => html`<li>${line}</li>`)}</ol>` : null}
    ${trainer.analysis?.length ? html`<section class="blackjack-analysis" aria-label="Bet analyzer">${trainer.analysis.map((line) => html`<p>${line}</p>`)}</section>` : null}
    ${trainer.quiz ? html`<section class="blackjack-quiz" aria-label="Card counting quiz">
      <p>${trainer.quiz.prompt}</p>
      <div>
        ${trainer.quiz.choices.map((choice) => html`<button type="button" class=${quizChoice === choice ? "selected" : ""} onClick=${() => setQuizChoice(choice)}>${choice}</button>`)}
      </div>
      ${quizChoice != null ? html`<strong>${quizChoice === trainer.quiz.answer ? "Correct" : `Count was ${trainer.quiz.answer}`}</strong>` : null}
    </section>` : null}
  `;
}

function ShoeVisualization({ shoe }) {
  const dealtPercent = (shoe.dealt_cards * 100) / Math.max(1, shoe.total_cards);
  const cutPercent = (shoe.cut_card * 100) / Math.max(1, shoe.total_cards);
  return html`<section class="blackjack-shoe" aria-label="Shoe visualization">
    <div class="blackjack-shoe-bar" role="img" aria-label=${`${shoe.dealt_cards} of ${shoe.total_cards} cards dealt; reshuffle at ${shoe.cut_card} cards`}>
      <span class="blackjack-shoe-dealt" style=${`width:${dealtPercent}%`}></span>
      <span class="blackjack-shoe-marker" style=${`left:${cutPercent}%`}></span>
    </div>
    <p class="blackjack-shoe-text">${shoe.decks} decks · ${shoe.dealt_cards} dealt · reshuffle at ${shoe.cut_card} (${shoe.penetration_percent}%) · ${shoe.remaining_cards} remaining · ${shoe.hands_dealt} rounds this shoe</p>
    ${shoe.fresh_shuffle ? html`<p class="blackjack-fresh-shuffle">Fresh shuffle.</p>` : null}
  </section>`;
}

function seatNote(seat, state) {
  if (seat.result) return seat.result;
  if (seat.leaving) return "Leaving";
  if (seat.waiting) return "Sitting out";
  if (state.phase === "betting") return seat.bet == null ? "Deciding…" : `Bet ${money(seat.bet)}`;
  if (state.phase === "insurance") return seat.insurance ? "Insured" : "Insurance?";
  return seat.bet == null ? "" : `Bet ${money(seat.bet)}`;
}

// Everybody else at the table: a compact tile per seat with their cards small,
// so the viewer's own hand keeps the room.
function Seat({ seat, state }) {
  const acting = state.phase === "playing" && state.current_seat === seat.index;
  return html`<article class=${`blackjack-seat${acting ? " acting" : ""}${seat.waiting ? " waiting" : ""}`} aria-label=${`${seat.display_name}'s seat`}>
    <header><b>${seat.display_name}</b><span>${money(seat.stack)}</span></header>
    <div class="blackjack-seat-hands">
      ${seat.hands.map((hand, index) => html`<div class=${`board${acting && state.current_hand === index ? " active" : ""}`} style=${`--card-count:${hand.cards.length}`}>${hand.cards.map((card) => html`<${Card} value=${card} />`)}<small>${hand.score}</small></div>`)}
    </div>
    <p class="blackjack-seat-note">${seatNote(seat, state)}</p>
    ${acting && state.deadline ? html`<${TurnClock} deadline=${state.deadline} duration=${state.turn_seconds * 1000} />` : null}
  </article>`;
}

function App() {
  const [state, setState] = useState(null);
  const [error, setError] = useState("");
  const [settings, setSettings] = useState(readTrainerSettings);
  const [quizChoice, setQuizChoice] = useState(null);
  const [pending, run] = usePending();
  const busy = pending != null;

  const post = (path, body = {}, then = null) => run(path, async () => {
    const response = await fetch(`/blackjack/tables/${tableId}/${path}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!response.ok) {
      setError(await responseError(response));
      return;
    }
    setError("");
    setState(await response.json());
    refreshBank().catch(() => {});
    if (then) then();
  });

  useEffect(() => {
    const load = () => fetch(`/blackjack/tables/${tableId}/state`)
      .then((response) => (response.ok ? response.json() : null))
      .then((next) => next && setState(next))
      .catch(() => {});
    load();
    const events = new EventSource(`/blackjack/tables/${tableId}/events`);
    events.addEventListener("state", (event) => setState(JSON.parse(event.data)));
    events.addEventListener("error", load);
    const syncBalance = (event) => {
      if (!event.detail) return;
      setState((current) => current && { ...current, bank_balance: event.detail.balance });
    };
    window.addEventListener("bank:updated", syncBalance);
    refreshBank().catch(() => {});
    return () => {
      events.close();
      window.removeEventListener("bank:updated", syncBalance);
    };
  }, []);
  useEffect(() => setQuizChoice(null), [state?.phase]);

  const seated = state?.viewer_seat != null;
  const trainerControls = html`<${TrainerSettings} settings=${settings} setSettings=${setSettings} onChange=${(next) => seated && post("settings", next)} />`;
  if (!state) {
    return html`<${CardSettings} interactive=${true} trigger=${false} children=${trainerControls} />
      <section class="blackjack-table"><div class="actions blackjack-actions"><span class="deal-broke">Loading table…</span></div></section>`;
  }
  const viewer = seated ? state.seats.find((seat) => seat.index === state.viewer_seat) : null;
  const others = state.seats.filter((seat) => seat.index !== state.viewer_seat);
  const myTurn = state.phase === "playing" && state.current_seat === state.viewer_seat;
  const onTheClock = Boolean(state.deadline) && (myTurn || (state.phase === "betting" && state.can_bet) || (state.phase === "insurance" && state.can_insure));
  const turnDuration = 1000 * (state.turn_seconds || 10);
  const actor = state.phase === "playing" && state.current_seat != null ? state.seats.find((seat) => seat.index === state.current_seat) : null;
  const waitingOnBets = state.phase === "betting" && seated && viewer.bet != null;
  const message = actor ? (myTurn ? "Your turn" : `${actor.display_name} to act`) : waitingOnBets ? "Waiting for the other players…" : (state.phase === "settled" && viewer?.result) || state.message;
  const broke = seated && !state.can_bet && state.phase === "betting" && viewer.bet == null && viewer.stack < state.min_bet;

  let actions;
  if (!seated) {
    actions = state.can_join
      ? [html`<button class="deal-action" type="button" disabled=${busy} aria-busy=${pending === "join"} onClick=${() => post("join", settings)}>Sit down · ${wholeDollarMoney(state.buy_in)}</button>`]
      : [html`<span class="deal-broke">You're seated at another blackjack table.</span>`];
  } else if (state.can_bet) {
    actions = state.bet_options.map((amount) => html`<button class="deal-action" type="button" disabled=${busy || amount > viewer.stack} aria-busy=${pending === "bet"} onClick=${() => post("bet", { amount })}>Bet ${wholeDollarMoney(amount)}</button>`);
  } else if (broke) {
    actions = state.can_rebuy
      ? [html`<button class="deal-action" type="button" disabled=${busy} aria-busy=${pending === "rebuy"} onClick=${() => post("rebuy")}>Add chips · ${wholeDollarMoney(state.buy_in - viewer.stack)}</button>`]
      : [html`<span class="deal-broke">Not enough chips for the ${wholeDollarMoney(state.min_bet)} minimum.</span>`];
  } else if (state.phase === "insurance" && state.can_insure) {
    actions = [
      html`<button type="button" disabled=${busy} aria-busy=${pending === "action"} onClick=${() => post("action", { kind: "insure" })}>Insurance</button>`,
      html`<button type="button" disabled=${busy} aria-busy=${pending === "action"} onClick=${() => post("action", { kind: "decline" })}>No insurance</button>`,
    ];
  } else if (myTurn) {
    actions = [["hit", "Hit"], ["stand", "Stand"], ["double", "Double"], ["split", "Split"]]
      .filter(([kind]) => state[`can_${kind}`])
      .map(([kind, label]) => html`<button type="button" disabled=${busy} aria-busy=${pending === "action"} onClick=${() => post("action", { kind })}>${label}</button>`);
  } else {
    actions = [html`<span class="deal-broke">${state.phase === "playing" ? "Waiting for your turn…" : state.phase === "settled" ? "Next round shortly…" : waitingOnBets ? "Bet placed" : "Waiting for the dealer…"}</span>`];
  }

  return html`
    <${CardSettings} interactive=${true} trigger=${false} children=${trainerControls} />
    <section class="blackjack-table" data-phase=${state.phase}>
      <div class="blackjack-status-row">
        <span><b>${money(state.max_bet)}</b> table max</span>
        <span><b>${money(seated ? viewer.stack : state.bank_balance)}</b> ${seated ? "your chips" : "bank"}</span>
        <span><b>${viewer?.bet == null ? "—" : money(viewer.bet)}</b> your bet</span>
      </div>
      <${ShoeVisualization} shoe=${state.shoe} />
      <div class="blackjack-play-area">
        <${DealerHand} cards=${state.dealer} score=${state.dealer_score} hidden=${state.dealer_hidden} />
        ${others.length > 0 ? html`<div class="blackjack-seats" aria-label="Other players">${others.map((seat) => html`<${Seat} seat=${seat} state=${state} />`)}</div>` : null}
        <div class="blackjack-own-hands" data-hand-count=${viewer?.hands.length || 0}>
          ${viewer?.hands.length
            ? viewer.hands.map((hand, index) => html`<${PlayerHand} hand=${hand} index=${index} count=${viewer.hands.length} active=${myTurn && state.current_hand === index} bet=${viewer.bet} deadline=${state.deadline} duration=${turnDuration} />`)
            : html`<p class="blackjack-own-note">${!seated ? "Watching the table" : viewer.result ? viewer.result : viewer.waiting && state.phase !== "betting" ? "Sitting this round out" : "Place a bet to be dealt in"}</p>`}
        </div>
      </div>
      <div class="blackjack-feedback">
        <p class=${`blitz-feedback${myTurn ? " blackjack-turn-announcement" : ""}`}>${message}</p>
        ${onTheClock && !myTurn ? html`<${TurnClock} deadline=${state.deadline} duration=${turnDuration} />` : null}
      </div>
      <${TrainerPanel} trainer=${state.trainer} quizChoice=${quizChoice} setQuizChoice=${setQuizChoice} />
      <div class="actions blackjack-actions" style=${`--action-count:${Math.max(1, actions.length)}`}>${actions}</div>
      <nav class="blackjack-controls">
        ${error ? html`<p class="error" role="alert">${error}</p>` : html`<span></span>`}
        ${seated && state.can_rebuy && !broke ? html`<button type="button" disabled=${busy} onClick=${() => post("rebuy")}>Add chips</button>` : null}
        ${seated ? html`<button type="button" disabled=${busy} onClick=${() => post("leave", {}, () => { location.href = "/blackjack"; })}>${viewer.bet == null ? "Leave table" : "Leave after this round"}</button>` : html`<a href="/blackjack">All tables</a>`}
      </nav>
    </section>
  `;
}

if (root) render(html`<${App} />`, root);
