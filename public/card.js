import { html } from "/public/vendor/htm-preact.js";

function cardFace(value) {
  const suitCode = value.slice(-1);
  const rawRank = value.slice(0, -1);
  const rank = rawRank === "T" ? "10" : rawRank;
  const suit = { h: "♥", d: "♦", c: "♣", s: "♠" }[suitCode] || suitCode;
  return { suitCode, rank, suit };
}

export function Card({ value, card, empty = false, hidden = false, interactive = false }) {
  if (empty) return html`<span class="playing-card empty-card" aria-hidden="true"></span>`;
  if (hidden) return html`<span class=${`playing-card card-back ${interactive ? "card-zoom-target" : ""}`} aria-label="Hidden card" tabindex=${interactive ? 0 : undefined}><i></i></span>`;
  const face = cardFace(value || card);
  const red = face.suitCode === "h" || face.suitCode === "d";
  // A face is rank over suit at one size: no pips, no court art, no second corner.
  return html`<span class=${`playing-card ${red ? "red" : "black"} ${interactive ? "card-zoom-target" : ""}`} aria-label=${value || card} tabindex=${interactive ? 0 : undefined}>
    <span class="card-corner"><b>${face.rank}</b><i>${face.suit}</i></span>
  </span>`;
}
