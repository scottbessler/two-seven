/**
 * The felt, and what a touch on it means.
 *
 * On a real table the bet *is* the chip's position: the middle of a square is
 * that number, the line between two squares is both, the cross where four meet
 * is all four. That is a lovely rule for a croupier's eye and a hostile one for
 * a thumb, so the board keeps the geometry honest and makes the aim visible
 * instead: press and the bet under your finger is named and its numbers light
 * up, slide until it is the one you wanted, lift to place the chip.
 *
 * `betAt` is the whole rule and it is pure -- a cell, and where in that cell
 * the touch landed, in and a bet id out. The ids it builds are the server's
 * own; `everyBet` below walks every zone of every cell so a drift between the
 * two shows up as a test failure rather than as a refused chip.
 */
import { html } from "/public/vendor/htm-preact.js";

export const ROWS = 12;
export const COLUMNS = 3;
/** How much of a cell, at each edge, belongs to the line rather than the square. */
export const EDGE = 0.27;

const number = (row, column) => row * COLUMNS + column + 1;
const ids = (...numbers) => numbers.toSorted((a, b) => a - b).join("-");

const straight = (n) => `straight:${n}`;
const split = (a, b) => `split:${ids(a, b)}`;
const street = (row) => `street:${ids(number(row, 0), number(row, 1), number(row, 2))}`;
const corner = (row, column) =>
  `corner:${ids(
    number(row, column),
    number(row, column + 1),
    number(row + 1, column),
    number(row + 1, column + 1),
  )}`;
const line = (row) =>
  `line:${ids(
    number(row, 0), number(row, 1), number(row, 2),
    number(row + 1, 0), number(row + 1, 1), number(row + 1, 2),
  )}`;

/** A fraction pulled into 0..1, so a press on a boundary still names a zone. */
function grip(value, size) {
  return Math.min(1, Math.max(0, value / size));
}

/** Which third of the cell a fraction falls in: -1 near the low edge, 1 near the high. */
function band(fraction) {
  if (fraction < EDGE) return -1;
  if (fraction > 1 - EDGE) return 1;
  return 0;
}

/**
 * The bet a touch at (`fx`, `fy`) within a numbered cell names. Fractions are
 * 0..1 across and down the cell. Returns null only for a spot no bet exists
 * for, which the caller treats as "no aim" rather than as an error.
 */
export function betAt(row, column, fx, fy) {
  const across = band(fx);
  const down = band(fy);
  const top = row === 0;
  const bottom = row === ROWS - 1;
  const left = column === 0;
  const right = column === COLUMNS - 1;

  if (across === 0 && down === 0) return straight(number(row, column));

  // A vertex: four squares meet, unless the board runs out first.
  if (across !== 0 && down !== 0) {
    const upper = down < 0;
    const outer = across < 0 ? left : right;
    if (upper && top) {
      // Against the zero: the outer corners are the first four, and the two
      // inner ones are the trios the zero makes with the first street.
      if (outer) return "basket:0-1-2-3";
      return across < 0 ? `trio:${ids(0, number(0, column - 1), number(0, column))}`
        : `trio:${ids(0, number(0, column), number(0, column + 1))}`;
    }
    if (!upper && bottom) return straight(number(row, column));
    const pair = upper ? row - 1 : row;
    // Along the outer edge a vertex is the six line across both rows; inside
    // the grid it is the corner of the four squares that meet there.
    if (outer) return line(pair);
    return corner(pair, across < 0 ? column - 1 : column);
  }

  // A vertical line: the split across it, or the street at the board's edge.
  if (across !== 0) {
    if (across < 0 && left) return street(row);
    if (across > 0 && right) return street(row);
    return split(number(row, column), number(row, column + across));
  }

  // A horizontal line: the split down it. The top of the first row is the
  // zero's own edge; the bottom of the last row has nothing below it.
  if (down < 0 && top) return split(0, number(0, column));
  if (down > 0 && bottom) return straight(number(row, column));
  return split(number(row, column), number(row + down, column));
}

/** The zero sits across all three columns, so its edge carries their bets. */
export function betAtZero(fx, fy) {
  if (band(fy) <= 0) return straight(0);
  const across = fx * COLUMNS;
  const column = Math.min(COLUMNS - 1, Math.floor(across));
  const within = across - column;
  const edge = band(within);
  if (edge === 0) return split(0, number(0, column));
  const outer = (edge < 0 && column === 0) || (edge > 0 && column === COLUMNS - 1);
  if (outer) return "basket:0-1-2-3";
  return edge < 0
    ? `trio:${ids(0, number(0, column - 1), number(0, column))}`
    : `trio:${ids(0, number(0, column), number(0, column + 1))}`;
}

/**
 * Every bet the geometry can name, from every zone of every cell. The test
 * that walks this against the server's catalogue is what keeps one board.
 */
export function everyBet() {
  const found = new Set();
  const thirds = [EDGE / 2, 0.5, 1 - EDGE / 2];
  for (const fx of thirds) {
    for (const fy of thirds) {
      found.add(betAtZero(fx, fy));
      for (let row = 0; row < ROWS; row++) {
        for (let column = 0; column < COLUMNS; column++) {
          found.add(betAt(row, column, fx, fy));
        }
      }
    }
  }
  found.delete(null);
  return [...found];
}

/** The outside bets, in the order they sit on the felt. */
export const DOZENS = [
  { id: "dozen:1", label: "1st 12" },
  { id: "dozen:2", label: "2nd 12" },
  { id: "dozen:3", label: "3rd 12" },
];
/* Two rows on a phone rather than the croupier's one, paired down the columns:
   low over high, even over odd, red over black. Each column is then one
   proposition and its opposite, which is how they are read anyway. */
export const EVENS = [
  { id: "low", label: "1–18" },
  { id: "even", label: "Even" },
  { id: "red", label: "Red", swatch: "red" },
  { id: "high", label: "19–36" },
  { id: "odd", label: "Odd" },
  { id: "black", label: "Black", swatch: "black" },
];
export const COLUMN_BETS = [
  { id: "column:1", label: "2 to 1" },
  { id: "column:2", label: "2 to 1" },
  { id: "column:3", label: "2 to 1" },
];

const REDS = new Set([1, 3, 5, 7, 9, 12, 14, 16, 18, 19, 21, 23, 25, 27, 30, 32, 34, 36]);
export function pocketColour(n) {
  return n === 0 ? "green" : REDS.has(n) ? "red" : "black";
}

/**
 * Where a bet's chip sits. A chip is not *in* a square unless the bet is that
 * one number: a split rides the line between two, a corner the cross where four
 * meet. Each bet therefore anchors to one cell and an edge of it, which puts
 * the chip exactly where a croupier would have pushed it -- and makes a layout
 * of ten chips readable at a glance instead of a list to cross-reference.
 */
export function anchorOf(id) {
  const [kind, list] = id.split(":");
  if (!list) return null;
  const numbers = list.split("-").map(Number);
  const [first, second] = numbers;
  switch (kind) {
    case "straight":
      return { number: first, place: "spot" };
    case "split":
      if (first === 0) return { number: second, place: "top" };
      return second === first + 1
        ? { number: first, place: "right" }
        : { number: first, place: "bottom" };
    case "street":
      return { number: first, place: "left" };
    case "corner":
      return { number: first, place: "corner-br" };
    case "line":
      return { number: first, place: "corner-bl" };
    case "trio":
      return { number: second, place: "corner-tr" };
    case "basket":
      return { number: 1, place: "corner-tl" };
    default:
      return null;
  }
}

function Chip({ amount, place, money }) {
  if (!amount) return null;
  // One disc carrying the total, rather than a pile: at this size a stack of
  // discs is unreadable and the number is the thing being asked about.
  return html`<span class=${`rl-chip at-${place}`}>${money(amount)}</span>`;
}

/**
 * The board. `aim` is the bet under the pointer right now, `spots` what is
 * already staked, and `lit` the numbers to highlight -- all decided by the
 * caller so this stays a drawing of state rather than a holder of it.
 */
export function Board({ aim, spots, lit, winner, money, onAim, onPlace, onCancel }) {
  const staked = new Map(spots.map((spot) => [spot.bet, spot.amount]));
  const glow = new Set(lit || []);
  // Chips that ride a line belong to the cell they are anchored against, so
  // each cell draws its own and nothing needs an overlay to line up with.
  const riding = new Map();
  for (const spot of spots) {
    const anchor = anchorOf(spot.bet);
    if (!anchor || anchor.place === "spot") continue;
    const held = riding.get(anchor.number) || [];
    held.push({ ...anchor, amount: spot.amount, bet: spot.bet });
    riding.set(anchor.number, held);
  }
  const ridingOn = (n) =>
    (riding.get(n) || []).map(
      (chip) => html`<${Chip} key=${chip.bet} amount=${chip.amount} place=${chip.place} money=${money} />`,
    );

  const zonesFrom = (event) => {
    // Not `event.target`: the press captures the pointer so the whole gesture
    // keeps arriving even when the thumb slides off the cell it started on, and
    // capture retargets every later event -- the release included -- to the
    // board itself. The point under the finger is the thing that stayed true.
    const cell = document.elementFromPoint(event.clientX, event.clientY)?.closest("[data-cell]");
    if (!cell) return null;
    const box = cell.getBoundingClientRect();
    // Clamped, not rejected: a press right on a boundary can resolve to either
    // of the cells that meet there, and the fraction then lands a hair outside
    // the one that answered. Both readings name the same line, so the cell
    // under the finger is taken as the truth and the fraction is pulled in.
    const fx = grip(event.clientX - box.left, box.width);
    const fy = grip(event.clientY - box.top, box.height);
    const at = cell.dataset.cell;
    if (at === "zero") return betAtZero(fx, fy);
    if (at.includes(",")) {
      const [row, column] = at.split(",").map(Number);
      return betAt(row, column, fx, fy);
    }
    return at;
  };

  const pointer = (event) => {
    // A press anywhere on the felt aims; the chip is not placed until it lifts,
    // so a thumb can slide onto the line it meant without spending anything.
    if (event.type === "pointerdown") event.currentTarget.setPointerCapture(event.pointerId);
    if (event.type === "pointermove" && !event.buttons && event.pointerType !== "mouse") return;
    onAim(zonesFrom(event));
  };

  const release = (event) => {
    const bet = zonesFrom(event);
    if (bet) onPlace(bet);
    onCancel();
  };

  return html`<div class="rl-board"
    onPointerDown=${pointer} onPointerMove=${pointer} onPointerUp=${release}
    onPointerCancel=${onCancel} onPointerLeave=${(event) => event.pointerType === "mouse" && onCancel()}>
    <div class=${`rl-zero ${winner === 0 ? "winner" : ""} ${glow.has(0) ? "lit" : ""}`} data-cell="zero">
      <span>0</span><${Chip} amount=${staked.get("straight:0")} place="spot" money=${money} />
    </div>
    ${Array.from({ length: ROWS }, (_row, row) =>
      Array.from({ length: COLUMNS }, (_column, column) => {
        const n = number(row, column);
        return html`<div key=${n}
          class=${`rl-cell ${pocketColour(n)} ${glow.has(n) ? "lit" : ""} ${winner === n ? "winner" : ""}`}
          style=${`grid-area:${row + 2}/${column + 1}`}
          data-cell=${`${row},${column}`}>
          <span>${n}</span>
          <${Chip} amount=${staked.get(straight(n))} place="spot" money=${money} />
          ${ridingOn(n)}
        </div>`;
      }),
    )}
    ${DOZENS.map(
      (dozen, index) => html`<div key=${dozen.id}
        class=${`rl-outside rl-dozen ${aim === dozen.id ? "aiming" : ""}`}
        style=${`grid-area:${2 + index * 4}/4/${6 + index * 4}/5`}
        data-cell=${dozen.id}>
        <span>${dozen.label}</span><${Chip} amount=${staked.get(dozen.id)} place="spot" money=${money} />
      </div>`,
    )}
    ${COLUMN_BETS.map(
      (bet, index) => html`<div key=${bet.id}
        class=${`rl-outside ${aim === bet.id ? "aiming" : ""}`}
        style=${`grid-area:14/${index + 1}`}
        data-cell=${bet.id}>
        <span>${bet.label}</span><${Chip} amount=${staked.get(bet.id)} place="spot" money=${money} />
      </div>`,
    )}
    ${EVENS.map(
      (bet, index) => html`<div key=${bet.id}
        class=${`rl-outside ${bet.swatch ? `rl-swatch ${bet.swatch}` : ""} ${aim === bet.id ? "aiming" : ""}`}
        style=${`grid-area:${15 + Math.floor(index / 3)}/${(index % 3) + 1}`}
        data-cell=${bet.id}>
        <span>${bet.label}</span><${Chip} amount=${staked.get(bet.id)} place="spot" money=${money} />
      </div>`,
    )}
  </div>`;
}
