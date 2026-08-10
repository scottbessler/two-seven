import { html, render, useEffect, useState } from "/public/vendor/htm-preact.js";

const root = document.getElementById("blitz-app");

function money(cents) {
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(cents);
  return `${sign}$${Math.floor(abs / 100).toLocaleString()}.${String(abs % 100).padStart(2, "0")}`;
}

function seconds(ms) {
  return `${(ms / 1000).toFixed(1)}s`;
}

function cardFace(value) {
  const suitCode = value.slice(-1);
  const rawRank = value.slice(0, -1);
  const rank = rawRank === "T" ? "10" : rawRank;
  const suit = { h: "♥", d: "♦", c: "♣", s: "♠" }[suitCode] || suitCode;
  const numeric = { A: 1, K: 13, Q: 12, J: 11, T: 10 }[rawRank] || Number(rawRank);
  return { suitCode, rank, suit, numeric };
}

const PIP_POSITIONS = {
  2: ["top-center", "bottom-center"],
  3: ["top-center", "middle-center", "bottom-center"],
  4: ["top-left", "top-right", "bottom-left", "bottom-right"],
  5: ["top-left", "top-right", "middle-center", "bottom-left", "bottom-right"],
  6: ["top-left", "top-right", "middle-left", "middle-right", "bottom-left", "bottom-right"],
  7: ["top-left", "top-right", "middle-left", "middle-right", "bottom-left", "bottom-right", "upper-center"],
  8: ["top-left", "top-right", "middle-left", "middle-right", "bottom-left", "bottom-right", "upper-center", "lower-center"],
  9: ["top-left", "top-right", "upper-left", "upper-right", "middle-center", "lower-left", "lower-right", "bottom-left", "bottom-right"],
  10: ["top-left", "top-right", "upper-left", "upper-right", "middle-left", "middle-right", "lower-left", "lower-right", "bottom-left", "bottom-right"],
};

function Card({ value }) {
  const { suitCode, rank, suit, numeric } = cardFace(value);
  const red = suitCode === "h" || suitCode === "d";
  const court = { 1: "A", 11: "J", 12: "Q", 13: "K" }[numeric];
  return html`<span class=${red ? "playing-card red" : "playing-card black"} aria-label=${value}>
    <span class="card-corner"><b>${rank}</b><i>${suit}</i></span>
    ${court
      ? html`<span class="card-art card-art-${court}"><i>${suit}</i><b>${court}</b></span>`
      : html`<span class="pip-grid pip-grid-${numeric}">${PIP_POSITIONS[numeric].map((position) => html`<i class=${`card-pip-${position}`}>${suit}</i>`)}</span>`}
    <span class="card-corner card-corner-bottom"><b>${rank}</b><i>${suit}</i></span>
  </span>`;
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [feedback, setFeedback] = useState("");
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(timer);
  }, []);

  const start = async (difficulty) => {
    setBusy(true);
    setError("");
    setFeedback("");
    const response = await fetch("/hand-blitz/start", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ difficulty }),
    });
    setBusy(false);
    if (!response.ok) {
      setError("Unable to start Hand Blitz");
      return;
    }
    const data = await response.json();
    setRun(data.run);
    setStats(data.stats);
  };

  const answer = async (choice) => {
    if (!run || busy || !run.active) return;
    setBusy(true);
    const response = await fetch("/hand-blitz/answer", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ run_id: run.id, round_id: run.round.id, choice }),
    });
    setBusy(false);
    if (!response.ok) {
      setError("That round is no longer available");
      return;
    }
    const data = await response.json();
    setRun(data.run);
    setStats(data.stats);
    setFeedback(data.correct ? `Correct: ${data.winning_label}` : `${data.timed_out ? "Time" : "Miss"}: ${data.winning_label}`);
  };

  if (!run) {
    return html`<section class="blitz-menu">
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
        ].map(([id, label, buyIn, limit]) => html`<button type="button" disabled=${busy} onClick=${() => start(id)}><b>${label}</b><span>${buyIn} buy-in · ${limit}</span></button>`)}
      </div>
      ${error && html`<p class="error">${error}</p>`}
    </section>`;
  }

  const remaining = Math.max(0, run.round.deadline_ms - now);
  const progress = Math.max(0, Math.min(100, (remaining / run.round.time_limit_ms) * 100));

  return html`<section class="blitz-table">
    <div class="blitz-score">
      <span><b>${run.correct}</b> correct</span>
      <span><b>${money(run.earnings)}</b> won</span>
      <span><b>${seconds(remaining)}</b> clock</span>
    </div>
    <div class="blitz-clock"><span style=${{ width: `${progress}%` }}></span></div>
    <${Cards} values=${run.round.board} />
    <div class="blitz-hands">
      ${run.round.hands.map((hand, index) => html`<button type="button" disabled=${busy || !run.active} onClick=${() => answer(index)} aria-label=${`Choose hand ${index + 1}`}>
        <span>Hand ${index + 1}</span>
        <${Cards} values=${hand} />
      </button>`)}
    </div>
    ${feedback && html`<p class=${run.active ? "blitz-feedback" : "error"}>${feedback}</p>`}
    ${!run.active && html`<div class="blitz-actions"><button type="button" onClick=${() => setRun(null)}>Play again</button></div>`}
  </section>`;
}

if (root) render(html`<${App} />`, root);
