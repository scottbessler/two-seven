import { html } from "/public/vendor/htm-preact.js";

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

function cardFace(value) {
  const suitCode = value.slice(-1);
  const rawRank = value.slice(0, -1);
  const rank = rawRank === "T" ? "10" : rawRank;
  const suit = { h: "♥", d: "♦", c: "♣", s: "♠" }[suitCode] || suitCode;
  const numeric = { A: 1, K: 13, Q: 12, J: 11, T: 10 }[rawRank] || Number(rawRank);
  return { suitCode, rank, suit, numeric };
}

function compactPipStyle(count, index) {
  const sideCount = Math.floor(count / 2);
  if (count % 2 === 1 && index === sideCount) return { "--compact-pip-left": "95%", "--compact-pip-top": "50%" };
  const lower = index > sideCount || (count % 2 === 0 && index >= sideCount);
  const sideIndex = lower ? index - sideCount - (count % 2) : index;
  const positions = sideCount === 1 ? [30] : Array.from({ length: sideCount }, (_, item) => 5 + (25 * item) / (sideCount - 1));
  return { "--compact-pip-left": lower ? "5%" : "95%", "--compact-pip-top": `${lower ? 100 - positions[sideIndex] : positions[sideIndex]}%` };
}

export function Card({ value, card, empty = false, hidden = false }) {
  if (empty) return html`<span class="playing-card empty-card" aria-hidden="true"></span>`;
  if (hidden) return html`<span class="playing-card card-back" aria-label="Hidden card" tabindex="0"><i></i></span>`;
  const face = cardFace(value || card);
  const court = { 1: "A", 11: "J", 12: "Q", 13: "K" }[face.numeric];
  const red = face.suitCode === "h" || face.suitCode === "d";
  const courtPiece = { J: "♘", Q: "♕", K: "♔" }[court];
  return html`<span class=${`playing-card ${red ? "red" : "black"}`} aria-label=${value || card} tabindex="0">
    <span class="card-corner"><b>${face.rank}</b><i>${face.suit}</i></span>
    <span class="card-frame">
      ${court
        ? court === "A"
          ? html`<span class="card-art card-art-A"><span class="ace-badge"><i>${face.suit}</i></span></span>`
          : html`<span class=${`card-art card-art-${court}`}><span class="court-piece">${courtPiece}</span><i>${face.suit}</i></span>`
        : html`<span class=${`pip-grid pip-grid-${face.numeric}`}>${PIP_POSITIONS[face.numeric].map((position, index) => html`<i class=${`card-pip-${position}`} style=${compactPipStyle(face.numeric, index)}>${face.suit}</i>`)}</span>`}
    </span>
    <span class="card-corner card-corner-bottom"><b>${face.rank}</b><i>${face.suit}</i></span>
  </span>`;
}
