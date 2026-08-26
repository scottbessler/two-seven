import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";
import { CardSettings } from "/public/card-settings.js";
import { money, refreshBank, responseError, usePending } from "/public/shared.js";
// Shared renderer contracts: rawRank === "T" ? "10", card-corner rank over suit.

const root = document.getElementById("blitz-app");

function seconds(ms) {
  return `${(ms / 1000).toFixed(1)}s`;
}

function Cards({ values }) {
  return html`<div class="board">${values.map((card) => html`<${Card} value=${card} />`)}</div>`;
}

function initialStats() {
  return {
    runs: Number(root.dataset.statsRuns || 0),
    attempts: Number(root.dataset.statsAttempts || 0),
    correct: Number(root.dataset.statsCorrect || 0),
    avg_answer_ms: Number(root.dataset.statsAvgMs || 0),
    best_streak: Number(root.dataset.statsBest || 0),
  };
}

function accuracy(stats) {
  return stats.attempts ? Math.floor((stats.correct * 100) / stats.attempts) : 0;
}

function App() {
  const [run, setRun] = useState(null);
  const [stats, setStats] = useState(initialStats());
  const [pending, runPending] = usePending();
  const busy = pending != null;
  const [error, setError] = useState("");
  const [feedback, setFeedback] = useState("");
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    fetch("/hand-blitz/resume", { headers: { Accept: "application/json" } })
      .then((response) => (response.ok ? response.json() : null))
      .then((savedRun) => savedRun && setRun(savedRun))
      .catch(() => {});
  }, []);

  const start = (difficulty) => runPending(`start:${difficulty}`, async () => {
    setError("");
    setFeedback("");
    const response = await fetch("/hand-blitz/start", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ difficulty }),
    });
    if (!response.ok) {
      setError(await responseError(response));
      return;
    }
    const data = await response.json();
    setRun(data.run);
    setStats(data.stats);
    refreshBank().catch(() => {});
  });

  const answer = (choice) => runPending(`answer:${choice}`, async () => {
    if (!run?.active) return;
    const response = await fetch("/hand-blitz/answer", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ run_id: run.id, round_id: run.round.id, choice }),
    });
    if (!response.ok) {
      setError(await responseError(response));
      return;
    }
    const data = await response.json();
    setRun(data.run);
    setStats(data.stats);
    setFeedback(data.correct ? `Correct: ${data.winning_label}` : `${data.timed_out ? "Time" : "Miss"}: ${data.winning_label}`);
    refreshBank().catch(() => {});
  });

  if (!run) {
    return html`<section class="blitz-menu">
      <${CardSettings} />
      <div class="blitz-stat-grid">
        <span><b>${stats.avg_answer_ms ? seconds(stats.avg_answer_ms) : "—"}</b> avg</span>
        <span><b>${accuracy(stats)}%</b> accuracy</span>
        <span><b>${stats.best_streak}</b> best</span>
      </div>
      <div class="difficulty-grid">
        ${[
          ["easy", "Easy", "$10.00", "20s"],
          ["normal", "Normal", "$50.00", "12s"],
          ["hard", "Hard", "$200.00", "6s"],
        ].map(([id, label, buyIn, limit]) => html`<button type="button" disabled=${busy} aria-busy=${pending === `start:${id}`} onClick=${() => start(id)}><b>${label}</b><span>${buyIn} buy-in · ${limit}</span></button>`)}
      </div>
      ${error && html`<p class="error">${error}</p>`}
    </section>`;
  }

  const remaining = Math.max(0, run.round.deadline_ms - now);
  const progress = Math.max(0, Math.min(100, (remaining / run.round.time_limit_ms) * 100));

  return html`<section class="blitz-table">
    <${CardSettings} />
    <div class="blitz-score">
      <span><b>${run.correct}</b> correct</span>
      <span><b>${money(run.earnings)}</b> won</span>
      <span><b>${seconds(remaining)}</b> clock</span>
    </div>
    <div class="blitz-clock"><span style=${{ width: `${progress}%` }}></span></div>
    <${Cards} values=${run.round.board} />
    <div class="blitz-hands">
      ${run.round.hands.map((hand, index) => html`<button type="button" disabled=${busy || !run.active} aria-busy=${pending === `answer:${index}`} onClick=${() => answer(index)} aria-label=${`Choose hand ${index + 1}`}>
        <span>Hand ${index + 1}</span>
        <${Cards} values=${hand} />
      </button>`)}
    </div>
    ${feedback && html`<p class=${run.active ? "blitz-feedback" : "error"}>${feedback}</p>`}
    ${!run.active && html`<div class="blitz-actions"><button type="button" onClick=${() => setRun(null)}>Play again</button></div>`}
  </section>`;
}

if (root) render(html`<${App} />`, root);
