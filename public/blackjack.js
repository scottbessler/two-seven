import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings } from "/public/card-settings.js";
import { money, refreshBank, responseError, wholeDollarMoney } from "/public/shared.js";
// Shared renderer contract: card-corner rank over suit.

const root = document.getElementById("blackjack-app");
const CHEAPEST_STARTING_BET_CAP = 10000;
const TRAINER_KEYS = {
  decks: "blackjack-trainer-decks",
  penetrationPercent: "blackjack-trainer-penetration-percent",
  countingTutor: "blackjack-counting-tutor",
  countingQuiz: "blackjack-counting-quiz",
  betAnalyzer: "blackjack-bet-analyzer",
};

// Mirrors bet_options and max_starting_bet in src/blackjack.rs: a nibble, a
// real bet, and a big one, capped at half the bankroll so a double or split
// remains affordable.
function betOptions(balance) {
  if (balance < 100) return [];
  const maxStart = Math.min(balance, Math.max(100, Math.floor(balance / 2 / 100) * 100));
  const rounded = [Math.min(balance / 100, CHEAPEST_STARTING_BET_CAP), balance / 20, balance / 4]
    .map((bet) => Math.min(maxStart, Math.max(100, Math.floor(bet / 100) * 100)));
  return [...new Set([...rounded, maxStart])].toSorted((left, right) => left - right);
}

function Hand({ title, cards, score, hidden }) {
  return html`<section class="blackjack-hand">
    <h2>${title}${score == null ? "" : ` · ${score}`}</h2>
    <div class="board">
      ${cards.map((card) => html`<${Card} value=${card} interactive=${true} />`)}
      ${hidden && html`<${Card} hidden=${true} interactive=${true} />`}
    </div>
  </section>`;
}

function readTrainerSettings() {
  const decks = Number(localStorage.getItem(TRAINER_KEYS.decks));
  const penetrationPercent = Number(localStorage.getItem(TRAINER_KEYS.penetrationPercent));
  return {
    decks: [1, 2, 8].includes(decks) ? decks : 8,
    penetrationPercent: penetrationPercent >= 25 && penetrationPercent <= 90 ? penetrationPercent : 50,
    countingTutor: localStorage.getItem(TRAINER_KEYS.countingTutor) === "on",
    countingQuiz: localStorage.getItem(TRAINER_KEYS.countingQuiz) === "on",
    betAnalyzer: localStorage.getItem(TRAINER_KEYS.betAnalyzer) === "on",
  };
}

function TrainerSettings({ settings, setSettings }) {
  const setNumber = (name, key) => (event) => {
    const value = Number(event.currentTarget.value);
    setSettings((current) => ({ ...current, [name]: value }));
    localStorage.setItem(key, String(value));
  };
  const toggle = (name, key) => (event) => {
    const value = event.currentTarget.checked;
    setSettings((current) => ({ ...current, [name]: value }));
    localStorage.setItem(key, value ? "on" : "off");
  };
  return html`
    <label><span>Blackjack decks <output>${settings.decks}</output></span><select name="blackjack-decks" value=${settings.decks} onChange=${setNumber("decks", TRAINER_KEYS.decks)}>
      <option value="1">Single deck</option>
      <option value="2">Double deck</option>
      <option value="8">Eight deck</option>
    </select></label>
    <label><span>Deck penetration <output>${settings.penetrationPercent}%</output></span><input name="blackjack-penetration-percent" type="number" min="25" max="90" step="1" value=${settings.penetrationPercent} onInput=${setNumber("penetrationPercent", TRAINER_KEYS.penetrationPercent)} /></label>
    <label class="card-option-toggle"><input name="counting-tutor" type="checkbox" checked=${settings.countingTutor} onChange=${toggle("countingTutor", TRAINER_KEYS.countingTutor)} /><span><b>Card counting tutor</b><small>Show the Hi-Lo running count and card-by-card changes</small></span></label>
    <label class="card-option-toggle"><input name="counting-quiz" type="checkbox" checked=${settings.countingQuiz} onChange=${toggle("countingQuiz", TRAINER_KEYS.countingQuiz)} /><span><b>Card counting quiz</b><small>Ask for the running count after each hand</small></span></label>
    <label class="card-option-toggle"><input name="bet-analyzer" type="checkbox" checked=${settings.betAnalyzer} onChange=${toggle("betAnalyzer", TRAINER_KEYS.betAnalyzer)} /><span><b>Bet analyzer</b><small>Compare your choices with basic strategy</small></span></label>
  `;
}

function TrainerPanel({ game, quizChoice, setQuizChoice }) {
  return html`
    ${game.count && html`<section class="blackjack-trainer-count" aria-label="Card counting tutor">
      <span><b>${game.count.running}</b> running</span>
      <span><b>${game.count.true_count.toFixed(1)}</b> true</span>
      <span><b>${game.count.penetration_percent}%</b> seen</span>
    </section>`}
    ${game.trainer_log?.length ? html`<ol class="blackjack-trainer-log" aria-label="Count log">${game.trainer_log.map((line) => html`<li>${line}</li>`)}</ol>` : null}
    ${game.analysis?.length ? html`<section class="blackjack-analysis" aria-label="Bet analyzer">${game.analysis.map((line) => html`<p>${line}</p>`)}</section>` : null}
    ${game.quiz && html`<section class="blackjack-quiz" aria-label="Card counting quiz">
      <p>${game.quiz.prompt}</p>
      <div>
        ${game.quiz.choices.map((choice) => html`<button type="button" class=${quizChoice === choice ? "selected" : ""} onClick=${() => setQuizChoice(choice)}>${choice}</button>`)}
      </div>
      ${quizChoice != null && html`<strong>${quizChoice === game.quiz.answer ? "Correct" : `Count was ${game.quiz.answer}`}</strong>`}
    </section>`}
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
    <p class="blackjack-shoe-text">${shoe.decks} decks · ${shoe.total_cards} cards · ${shoe.dealt_cards} dealt · reshuffle at ${shoe.cut_card} (${shoe.penetration_percent}%) · ${shoe.remaining_cards} remaining · ${shoe.hands_dealt} hands this shoe</p>
    ${shoe.fresh_shuffle && html`<p class="blackjack-fresh-shuffle">Fresh shuffle.</p>`}
  </section>`;
}

function App() {
  const [game, setGame] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [balance, setBalance] = useState(0);
  const [trainerSettings, setTrainerSettings] = useState(readTrainerSettings);
  const [quizChoice, setQuizChoice] = useState(null);

  useEffect(() => {
    const syncBalance = (event) => {
      if (event.detail) setBalance(event.detail.balance);
    };
    window.addEventListener("bank:updated", syncBalance);
    refreshBank().catch(() => {});
    fetch("/blackjack/resume")
      .then((response) => response.ok ? response.json() : null)
      .then((value) => value && setGame(value))
      .catch(() => {});
    return () => window.removeEventListener("bank:updated", syncBalance);
  }, []);

  useEffect(() => setQuizChoice(null), [game?.id, game?.status]);

  const start = async (amount) => {
    setBusy(true);
    setError("");
    const response = await fetch("/blackjack/start", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        bet: amount,
        settings: {
          decks: trainerSettings.decks,
          penetration_percent: trainerSettings.penetrationPercent,
          counting_tutor: trainerSettings.countingTutor,
          counting_quiz: trainerSettings.countingQuiz,
          bet_analyzer: trainerSettings.betAnalyzer,
        },
      }),
    });
    setBusy(false);
    if (!response.ok) {
      setError(await responseError(response));
      return;
    }
    setGame(await response.json());
    refreshBank().catch(() => {});
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
    refreshBank().catch(() => {});
  };

  const bets = betOptions(balance);
  const trainerControls = html`<${TrainerSettings} settings=${trainerSettings} setSettings=${setTrainerSettings} />`;
  return html`
    <${CardSettings} interactive=${true} trigger=${false} children=${trainerControls} />
    <section class="blackjack-table">
      ${game && html`
        <div class="blackjack-status-row">
          <span><b>${money(game.bet)}</b> bet</span>
          <span><b>${game.payout ? money(game.payout) : "—"}</b> payout</span>
          <span><b>${game.status}</b> status</span>
        </div>
        <${ShoeVisualization} shoe=${game.shoe} />
        <div class="blackjack-play-area">
          <${Hand} title="Dealer" cards=${game.dealer} score=${game.dealer_score} hidden=${game.dealer_score == null} />
          ${game.hands.map((hand, index) => html`<${Hand} title=${`Hand ${index + 1}${index === game.active_hand ? " · Active" : ""}`} cards=${hand.cards} score=${hand.score} />`)}
        </div>
        <p class="blitz-feedback">${game.message}</p>
        <${TrainerPanel} game=${game} quizChoice=${quizChoice} setQuizChoice=${setQuizChoice} />
      `}
      <div class="actions blackjack-actions">
        ${game?.status === "Playing" ? html`
          ${game.can_hit && html`<button type="button" disabled=${busy} onClick=${() => act("hit")}>Hit</button>`}
          ${game.can_stand && html`<button type="button" disabled=${busy} onClick=${() => act("stand")}>Stand</button>`}
          ${game.can_double && html`<button type="button" disabled=${busy} onClick=${() => act("double")}>Double</button>`}
          ${game.can_split && html`<button type="button" disabled=${busy} onClick=${() => act("split")}>Split</button>`}
          ${game.can_insure && html`<button type="button" disabled=${busy} onClick=${() => act("insurance")}>Insurance</button>`}
        ` : bets.length === 0
          ? html`<span class="deal-broke">Re-up from the coin menu to play a hand.</span>`
          : bets.map((amount) => html`<button class="deal-action" type="button" disabled=${busy} onClick=${() => start(amount)}>Deal ${wholeDollarMoney(amount)}</button>`)}
      </div>
      ${error && html`<p class="error">${error}</p>`}
    </section>
  `;
}

if (root) render(html`<${App} />`, root);
