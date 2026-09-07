import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings } from "/public/card-settings.js";
import { money, refreshBank, responseError, usePending, wholeDollarMoney } from "/public/shared.js";

const root = document.getElementById("blackjack-app");
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

function PlayerHand({ hand, index, count, active }) {
  const name = count > 1 ? `Hand ${index + 1}` : "You";
  return html`<section class=${`blackjack-player-hand${active ? " active" : ""}`} aria-label=${`${name}, ${hand.score}${active ? ", your turn" : ""}`}>
    <div class="board" style=${`--card-count:${hand.cards.length}`}>
      ${hand.cards.map((card) => html`<${Card} value=${card} interactive=${true} />`)}
    </div>
    <div class="blackjack-player-summary">
      <span>${name}</span>
      <strong>${hand.score}</strong>
      ${hand.bet == null ? null : html`<small>Bet ${money(hand.bet)}</small>`}
    </div>
  </section>`;
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
  if (!shoe) return null;
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

// Sitting down costs nothing: the slider picks a ceiling, and the ceiling is
// only the ladder the five wagers are cut from. The last rung is the whole
// bank, so it cannot be dragged past what the player actually has.
function SitDown({ state, busy, pending, onSit }) {
  const rungs = state.max_bets;
  // The slider starts at the cheapest seat: the ceiling is what you are asking
  // to be able to lose, so the safe end is the one to have to drag away from.
  const [index, setIndex] = useState(0);
  const pick = Math.min(index, Math.max(0, rungs.length - 1));
  const maxBet = rungs[pick];
  if (!rungs.length) {
    return html`<section class="blackjack-sit" aria-label="Sit down">
      <p class="deal-broke">You need ${wholeDollarMoney(state.max_bet)} in the bank to sit down.</p>
    </section>`;
  }
  return html`<section class="blackjack-sit" aria-label="Sit down">
    <div class="blackjack-sit-stakes">
      <span><b>${wholeDollarMoney(maxBet)}</b> max bet</span>
      <span><b>${wholeDollarMoney(maxBet / 5)}</b> min bet</span>
    </div>
    <input
      class="blackjack-sit-slider"
      type="range"
      min="0"
      max=${rungs.length - 1}
      step="1"
      value=${pick}
      aria-label="Maximum bet"
      aria-valuetext=${`${wholeDollarMoney(maxBet)} max bet`}
      onInput=${(event) => setIndex(Number(event.currentTarget.value))}
    />
    <div class="blackjack-sit-scale"><span>${wholeDollarMoney(rungs[0])}</span><span>${wholeDollarMoney(rungs[rungs.length - 1])}</span></div>
    <p class="blackjack-sit-note">Five wagers, ${wholeDollarMoney(maxBet / 5)} to ${wholeDollarMoney(maxBet)}.</p>
    <div class="actions blackjack-actions">
      <button class="deal-action" type="button" disabled=${busy} aria-busy=${pending === "sit"} onClick=${() => onSit(maxBet)}>Sit down</button>
    </div>
  </section>`;
}

function App() {
  const [state, setState] = useState(null);
  const [error, setError] = useState("");
  const [settings, setSettings] = useState(readTrainerSettings);
  const [quizChoice, setQuizChoice] = useState(null);
  const [pending, run] = usePending();
  const busy = pending != null;

  const post = (path, body = {}) => run(path, async () => {
    const response = await fetch(`/blackjack/${path}`, {
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
  });

  useEffect(() => {
    // Nobody else can change this game, so there is nothing to subscribe to:
    // the state is loaded once and every action answers with the next one.
    fetch("/blackjack/state")
      .then((response) => (response.ok ? response.json() : null))
      .then((next) => next && setState(next))
      .catch(() => {});
    const syncBalance = (event) => {
      if (!event.detail) return;
      setState((current) => current && { ...current, bank_balance: event.detail.balance });
    };
    window.addEventListener("bank:updated", syncBalance);
    refreshBank().catch(() => {});
    return () => window.removeEventListener("bank:updated", syncBalance);
  }, []);
  useEffect(() => setQuizChoice(null), [state?.phase]);

  const seated = Boolean(state?.seated);
  const trainerControls = html`<${TrainerSettings} settings=${settings} setSettings=${setSettings} onChange=${(next) => seated && post("settings", next)} />`;
  if (!state) {
    return html`<${CardSettings} interactive=${true} trigger=${false} children=${trainerControls} />
      <section class="blackjack-table"><div class="actions blackjack-actions"><span class="deal-broke">Loading your table…</span></div></section>`;
  }

  if (!seated) {
    return html`
      <${CardSettings} interactive=${true} trigger=${false} children=${trainerControls} />
      <section class="blackjack-table" data-phase="sitting">
        <div class="blackjack-status-row">
          <span><b>${money(state.bank_balance)}</b> bank</span>
        </div>
        <h1 class="blackjack-sit-heading">Choose your maximum bet</h1>
        <${SitDown} state=${state} busy=${busy} pending=${pending} onSit=${(maxBet) => post("sit", { max_bet: maxBet, ...settings })} />
        ${error ? html`<p class="error" role="alert">${error}</p>` : null}
      </section>`;
  }

  const myTurn = state.phase === "playing";
  const broke = state.phase !== "playing" && state.bet == null && state.bank_balance < state.min_bet;

  let actions;
  if (broke) {
    actions = [html`<span class="deal-broke">Not enough in the bank for the ${wholeDollarMoney(state.min_bet)} minimum.</span>`];
  } else if (state.can_bet) {
    actions = state.bet_options.map((amount) => html`<button class="deal-action" type="button" disabled=${busy || amount > state.bank_balance} aria-busy=${pending === "bet"} onClick=${() => post("bet", { amount })}>Bet ${wholeDollarMoney(amount)}</button>`);
  } else if (state.can_insure) {
    actions = [
      html`<button type="button" disabled=${busy} aria-busy=${pending === "action"} onClick=${() => post("action", { kind: "insure" })}>Insurance</button>`,
      html`<button type="button" disabled=${busy} aria-busy=${pending === "action"} onClick=${() => post("action", { kind: "decline" })}>No insurance</button>`,
    ];
  } else if (myTurn) {
    actions = [["hit", "Hit"], ["stand", "Stand"], ["double", "Double"], ["split", "Split"]]
      .filter(([kind]) => state[`can_${kind}`])
      .map(([kind, label]) => html`<button type="button" disabled=${busy} aria-busy=${pending === "action"} onClick=${() => post("action", { kind })}>${label}</button>`);
  } else {
    actions = [html`<span class="deal-broke">Waiting for the dealer…</span>`];
  }

  return html`
    <${CardSettings} interactive=${true} trigger=${false} children=${trainerControls} />
    <section class="blackjack-table" data-phase=${state.phase}>
      <div class="blackjack-status-row">
        <span><b>${money(state.max_bet)}</b> table max</span>
        <span><b>${money(state.bank_balance)}</b> your bank</span>
        <span><b>${state.bet == null ? "—" : money(state.bet)}</b> your bet</span>
      </div>
      <${ShoeVisualization} shoe=${state.shoe} />
      <div class="blackjack-play-area">
        <${DealerHand} cards=${state.dealer} score=${state.dealer_score} hidden=${state.dealer_hidden} />
        <div class="blackjack-own-hands" data-hand-count=${state.hands.length}>
          ${state.hands.length
            ? state.hands.map((hand, index) => html`<${PlayerHand} hand=${hand} index=${index} count=${state.hands.length} active=${myTurn && state.current_hand === index} />`)
            : html`<p class="blackjack-own-note">Place a bet to be dealt in</p>`}
        </div>
      </div>
      <div class="blackjack-feedback">
        <p class=${`blitz-feedback${myTurn ? " blackjack-turn-announcement" : ""}`}>${state.message}</p>
      </div>
      <${TrainerPanel} trainer=${state.trainer} quizChoice=${quizChoice} setQuizChoice=${setQuizChoice} />
      <div class=${`actions blackjack-actions${state.can_bet ? " blackjack-bet-row" : ""}`} style=${`--action-count:${Math.max(1, actions.length)}`}>${actions}</div>
      <nav class="blackjack-controls">
        ${error ? html`<p class="error" role="alert">${error}</p>` : html`<span></span>`}
        ${state.can_leave ? html`<button type="button" disabled=${busy} onClick=${() => post("leave")}>Leave table</button>` : null}
      </nav>
    </section>
  `;
}

if (root) render(html`<${App} />`, root);
