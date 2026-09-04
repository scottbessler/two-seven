import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings } from "/public/card-settings.js";
import { money, refreshBank, responseError, usePending, useResultClock, wholeDollarMoney } from "/public/shared.js";

const root = document.getElementById("blackjack-app");
const tableId = root?.dataset.tableId;
const keys = { counting_tutor: "blackjack-counting-tutor", counting_quiz: "blackjack-counting-quiz", bet_analyzer: "blackjack-bet-analyzer" };
const storedSettings = () => Object.fromEntries(Object.entries(keys).map(([name, key]) => [name, localStorage.getItem(key) === "on"]));

function Hand({ title, cards, score, hidden, compact }) {
  return html`<section class=${`blackjack-hand${compact ? " compact" : ""}`}><h2>${title}${score == null ? "" : ` · ${score}`}</h2><div class="board">${(cards || []).map((value) => html`<${Card} value=${value} />`)}${hidden && html`<${Card} hidden=${true} />`}</div></section>`;
}
function ShoeVisualization({ shoe }) {
  return html`<section class="blackjack-shoe"><div class="blackjack-shoe-bar"><i style=${{ width: `${100 * shoe.dealt_cards / shoe.total_cards}%` }}></i></div><p>${shoe.decks} decks · ${shoe.dealt_cards} dealt · ${shoe.remaining_cards} remaining</p></section>`;
}
function TrainerPanel({ trainer }) {
  if (!trainer) return null;
  return html`<section class="blackjack-trainer">${trainer.count && html`<p class="blackjack-trainer-count">Running count <b>${trainer.count.running}</b> · True count <b>${trainer.count.true_count.toFixed(1)}</b></p>`}${trainer.log?.length && html`<ol class="blackjack-trainer-log">${trainer.log.map((line) => html`<li>${line}</li>`)}</ol>`}${trainer.analysis?.map((line) => html`<p class="blackjack-analysis">${line}</p>`)}${trainer.quiz && html`<section class="blackjack-quiz"><p>${trainer.quiz.prompt}</p>${trainer.quiz.choices.map((choice) => html`<button type="button">${choice}</button>`)}</section>`}</section>`;
}
function TurnClock({ deadline, duration }) {
  const remaining = useResultClock(Boolean(deadline), deadline, duration);
  return html`<span class="turn-clock blackjack-turn-clock"><i style=${{ width: `${100 * remaining / duration}%` }}></i></span>`;
}
function Seat({ seat, state }) {
  const active = state.current_seat === seat.index;
  return html`<article class=${`blackjack-seat${active ? " acting" : ""}`}><header><b>${seat.display_name}</b><span>${money(seat.stack)} · ${seat.bet == null ? "No bet" : money(seat.bet)}</span></header>${seat.waiting && html`<small>Sitting out</small>`}${seat.leaving && html`<small>Leaving</small>`}${seat.hands.map((hand, index) => html`<${Hand} title=${`Hand ${index + 1}`} cards=${hand.cards} score=${hand.score} compact=${true} />`)}${seat.result && html`<p>${seat.result}</p>`}${active && state.phase === "playing" && html`<${TurnClock} deadline=${state.deadline} duration=${state.turn_seconds * 1000} />`}</article>`;
}
function Settings({ value, setValue, change }) {
  return html`${Object.keys(keys).map((name) => html`<label><input type="checkbox" checked=${value[name]} onChange=${(event) => { const next = { ...value, [name]: event.currentTarget.checked }; setValue(next); localStorage.setItem(keys[name], next[name] ? "on" : "off"); change(next); }} /> ${name.replaceAll("_", " ")}</label>`)}`;
}
function App() {
  const [state, setState] = useState(null);
  const [error, setError] = useState("");
  const [settings, setSettings] = useState(storedSettings);
  const [pending, run] = usePending();
  const seated = state?.viewer_seat != null;
  const viewer = seated ? state.seats.find((seat) => seat.index === state.viewer_seat) : null;
  const post = (path, body = {}) => run(path, async () => {
    const response = await fetch(`/blackjack/tables/${tableId}/${path}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    if (!response.ok) return setError(await responseError(response));
    setError(""); setState(await response.json()); refreshBank().catch(() => {});
  });
  useEffect(() => {
    const load = () => fetch(`/blackjack/tables/${tableId}/state`).then((response) => response.ok && response.json()).then((next) => next && setState(next)).catch(() => {});
    load();
    const events = new EventSource(`/blackjack/tables/${tableId}/events`);
    events.addEventListener("state", (event) => setState(JSON.parse(event.data)));
    events.addEventListener("error", load);
    const bank = (event) => setState((current) => current && { ...current, bank_balance: event.detail.balance });
    window.addEventListener("bank:updated", bank); refreshBank().catch(() => {});
    return () => { events.close(); window.removeEventListener("bank:updated", bank); };
  }, []);
  if (!state) return html`<section class="blackjack-table"><p>Loading table…</p></section>`;
  const change = (next) => seated && post("settings", next);
  const actions = [];
  if (state.can_bet) {
    for (const amount of state.bet_options) actions.push(html`<button class="deal-action" disabled=${pending || amount > viewer.stack} onClick=${() => post("bet", { amount })}>Bet ${wholeDollarMoney(amount)}</button>`);
  } else if (state.phase === "insurance" && state.can_insure) {
    actions.push(html`<button onClick=${() => post("action", { kind: "insure" })}>Insurance</button>`, html`<button onClick=${() => post("action", { kind: "decline" })}>No insurance</button>`);
  } else if (state.phase === "playing" && state.current_seat === state.viewer_seat) {
    for (const [kind, label] of [["hit", "Hit"], ["stand", "Stand"], ["double", "Double"], ["split", "Split"]]) {
      if (state[`can_${kind}`]) actions.push(html`<button onClick=${() => post("action", { kind })}>${label}</button>`);
    }
  } else {
    actions.push(html`<span class="deal-broke">${state.message || "Waiting for the dealer…"}</span>`);
  }
  return html`<${CardSettings} trigger=${false}><${Settings} value=${settings} setValue=${setSettings} change=${change} /></${CardSettings}><section class="blackjack-table">
    <div class="blackjack-status-row"><span>Table max <b>${money(state.max_bet)}</b></span><span>${seated ? "Your chips" : "Bank"} <b>${money(seated ? viewer.stack : state.bank_balance)}</b></span><span>${state.message}</span></div>
    <${ShoeVisualization} shoe=${state.shoe} /><div class="blackjack-play-area"><${Hand} title="Dealer" cards=${state.dealer} score=${state.dealer_score} hidden=${state.dealer_hidden} /><div class="blackjack-seats">${state.seats.filter((seat) => seat.index !== state.viewer_seat).map((seat) => html`<${Seat} seat=${seat} state=${state} />`)}</div>${viewer?.waiting && html`<p class="deal-broke">Sitting out</p>`}${viewer?.hands.map((hand, index) => html`<${Hand} title=${`Your hand ${index + 1}${state.current_seat === state.viewer_seat && state.current_hand === index ? " · Active" : ""}`} cards=${hand.cards} score=${hand.score} />`)}</div>
    <p class="blitz-feedback">${state.message}</p><${TrainerPanel} trainer=${state.trainer} /><div class="actions blackjack-actions" style=${`--action-count:${Math.max(1, actions.length)}`}>${state.deadline && ["betting", "insurance"].includes(state.phase) && (state.can_bet || state.can_insure) && html`<${TurnClock} deadline=${state.deadline} duration=${state.turn_seconds * 1000} />`}
      ${!seated && state.can_join && html`<button class="deal-action" onClick=${() => post("join", settings)}>Sit down · ${wholeDollarMoney(state.buy_in)}</button>`}${!seated && !state.can_join && html`<span class="deal-broke">You're seated at another blackjack table.</span>`}${actions}</div>
    <div class="table-controls">${seated && html`<button onClick=${async () => { await post("leave"); location.href = "/blackjack"; }}>Leave table</button>`}${state.can_rebuy && html`<button onClick=${() => post("rebuy")}>Add chips</button>`}</div>${error && html`<p class="error">${error}</p>`}
  </section>`;
}
if (root) render(html`<${App} />`, root);
