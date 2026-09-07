/**
 * The roulette table.
 *
 * The server owns the money and the pocket: a spin returns the number it has
 * already settled, and this page's job is to not give it away early. The wheel
 * is handed the number and the seed the server drew, plays the eight seconds it
 * takes to get there, and only then are the winnings shown -- so the animation
 * is the reveal rather than a decoration over one.
 *
 * Bets are the other half. `roulette-board.js` decides which bet a touch names;
 * everything here is what it costs, what it pays, and whether the stack covers
 * it -- and none of those are decided here either, because the board's odds and
 * the table's money both arrive from the server with the page.
 */
import { html, render, useEffect, useRef, useState } from "/public/vendor/htm-preact.js";
import { money, refreshBank, responseError, usePending } from "/public/shared.js";
import { Board, pocketColour } from "/public/roulette-board.js";
import { createRouletteSound } from "/public/roulette-sound.js";
import { createWheel } from "/public/roulette-wheel.js";

const root = document.getElementById("roulette-app");
const read = (id, fallback) => {
  try {
    return JSON.parse(document.getElementById(id).textContent);
  } catch {
    return fallback;
  }
};
/** Every legal bet, priced by the server. The one source of odds on this page. */
const BOARD = new Map(read("roulette-board", []).map((spec) => [spec.id, spec]));

async function post(path, body) {
  const response = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify(body ?? {}),
  });
  if (!response.ok) throw new Error(await responseError(response));
  return response.json();
}

function Marquee({ history }) {
  if (history.length === 0) {
    return html`<p class="rl-empty">No spins yet.</p>`;
  }
  return html`<ol class="rl-marquee">
    ${history.map((n, index) => html`<li key=${index} class=${pocketColour(n)}>${n}</li>`)}
  </ol>`;
}

/** What the last spin did, once the wheel has caught up with it. */
function Result({ spin }) {
  const net = spin.returned - spin.staked;
  return html`<div class="rl-result">
    <span class=${`rl-pocket ${spin.colour}`}>${spin.number}</span>
    <div>
      <b class=${net >= 0 ? "rl-up" : "rl-down"}>${net >= 0 ? `+${money(net)}` : money(net)}</b>
      <span>${spin.wins.length
        ? `${spin.wins.length} winning ${spin.wins.length === 1 ? "bet" : "bets"} · ${money(spin.returned)} back`
        : `${money(spin.staked)} down`}</span>
    </div>
  </div>`;
}

function BuyInDialog({ amounts, index, pending, onIndex, onCancel, onConfirm }) {
  const amount = amounts[index];
  return html`<div class="rl-buy-in-backdrop" onClick=${onCancel}>
  <dialog class="rl-buy-in-dialog" open aria-labelledby="rl-buy-in-title"
    onClick=${(event) => event.stopPropagation()}>
    <form method="dialog" onSubmit=${(event) => {
      event.preventDefault();
      onConfirm(amount);
    }}>
      <header>
        <h2 id="rl-buy-in-title">Choose your stack</h2>
        <button class="rl-dialog-close" type="button" aria-label="Cancel buy-in" onClick=${onCancel}>×</button>
      </header>
      <p>Your chips stay at this table until you cash out.</p>
      <output for="roulette-buy-in">${money(amount)}</output>
      <input id="roulette-buy-in" type="range" min="0" max=${amounts.length - 1} step="1"
        value=${index} onInput=${(event) => onIndex(Number(event.currentTarget.value))} />
      <div class="rl-buy-in-rungs" aria-hidden="true">
        ${amounts.map((value) => html`<span key=${value}>${money(value).replace(".00", "")}</span>`)}
      </div>
      <footer>
        <button type="button" onClick=${onCancel}>Cancel</button>
        <button type="submit" disabled=${pending}>Buy in for ${money(amount)}</button>
      </footer>
    </form>
  </dialog></div>`;
}

function App() {
  const frame = useRef(null);
  const wheel = useRef(null);
  const sound = useRef(null);
  const [table, setTable] = useState(() => read("roulette-state", null));
  const [chip, setChip] = useState(() => (table?.chips ?? [100])[1] ?? 100);
  const [aim, setAim] = useState(null);
  const [error, setError] = useState("");
  const [pending, run] = usePending();
  // The number the server has already drawn, held back until the ball lands.
  const [spinning, setSpinning] = useState(false);
  const [wheelOpen, setWheelOpen] = useState(false);
  const [buyInOpen, setBuyInOpen] = useState(false);
  const [buyInIndex, setBuyInIndex] = useState(0);
  const settled = useRef(null);
  const [shown, setShown] = useState(() => read("roulette-state", null)?.last ?? null);

  useEffect(() => {
    sound.current = createRouletteSound();
    wheel.current = createWheel(frame.current, {
      tilt: 0.8,
      onImpact: (impact) => sound.current.strike(impact.kind, impact.strength),
      onFrame: ({ speed, resting }) => sound.current.roll(resting ? 0 : speed),
      onResult: () => {
        sound.current.stop();
        // The wheel has arrived where the server already was: now it can be said.
        if (settled.current) setShown(settled.current);
        settled.current = null;
        setSpinning(false);
        refreshBank();
      },
    });
    window.rouletteWheel = wheel.current;
    return () => {
      wheel.current.destroy();
      delete window.rouletteWheel;
    };
  }, []);

  const apply = (next) => {
    setTable(next);
    setError("");
    return next;
  };

  const act = (key, path, body) =>
    run(key, async () => {
      try {
        apply(await post(path, body));
      } catch (failure) {
        setError(failure.message);
      }
    });

  const place = (bet) => {
    if (spinning || pending) return;
    const spec = BOARD.get(bet);
    if (!spec) return;
    if (chip > table.available) {
      setError("You do not have the chips for that.");
      return;
    }
    act("place", "/roulette/bet", { bet, amount: chip });
  };

  const spin = () =>
    run("spin", async () => {
      try {
        const next = await post("/roulette/spin");
        setError("");
        // The stack is updated now -- it is the server's answer -- but the
        // number stays behind the wheel until the ball is in the pocket.
        setTable(next);
        setShown(null);
        settled.current = next.last;
        setSpinning(true);
        setWheelOpen(true);
        sound.current.begin();
        wheel.current.spin({ number: next.last.number, seed: next.last.seed });
      } catch (failure) {
        setError(failure.message);
      }
    });

  const buyIn = (amount) =>
    run("buy-in", async () => {
      try {
        apply(await post("/roulette/buy-in", { amount }));
        setBuyInOpen(false);
        refreshBank();
      } catch (failure) {
        setError(failure.message);
      }
    });

  if (!table) return html`<p class="error">This table could not be opened.</p>`;

  const seated = table.stack > 0 || table.staked > 0;
  const aimed = aim ? BOARD.get(aim) : null;
  const busy = pending != null || spinning;
  const buyIns = table.buy_ins ?? [table.buy_in];

  return html`<div class="rl-table">
    <section class="rl-dashboard" aria-label="Table status">
      <div class="rl-readout" role="status">
        ${aimed
          ? html`<span class="rl-aim"><b>${aimed.numbers.length > 6 ? `${aimed.numbers.length} numbers` : aimed.numbers.join(" · ")}</b>
              <span>${aimed.label} · pays ${aimed.payout} to 1</span></span>`
          : spinning
            ? html`<span class="rl-waiting">No more bets…</span>`
            : shown
              ? html`<${Result} spin=${shown} />`
              : html`<span class="rl-waiting rl-aim">Press the felt to aim, lift to place a chip</span>`}
      </div>
      <div class=${`rl-wheel-dock ${wheelOpen ? "open" : ""} ${spinning ? "spinning" : ""}`}>
        <button class="rl-wheel-trigger" type="button" disabled=${busy || table.staked === 0}
          aria-label=${spinning ? "Roulette wheel spinning" : "Spin the roulette wheel"} onClick=${spin}>
          <canvas class="roulette-canvas" ref=${frame} aria-hidden="true"></canvas>
          <span>${spinning ? "Spinning…" : table.staked ? "Spin" : "Place a bet"}</span>
        </button>
        ${wheelOpen && !spinning
          ? html`<button class="rl-wheel-close" type="button" aria-label="Close wheel"
              onClick=${() => setWheelOpen(false)}>×</button>`
          : null}
      </div>
      <div class="rl-summary">
        <${Marquee} history=${table.history} />
        <div class="rl-money">
          <span><b>${money(table.stack)}</b> stack</span>
          <span><b>${money(table.staked)}</b> on felt</span>
          <span><b>${money(table.available)}</b> to bet</span>
          <button class="rl-cash-out" type="button" disabled=${busy || !seated || table.staked > 0}
            onClick=${() => act("cash-out", "/roulette/cash-out")}>Cash out</button>
        </div>
      </div>
    </section>

    <div class="rl-stage">
      <div class="rl-felt">
      ${seated
        ? html`<${Board} aim=${aim} spots=${table.spots} lit=${aimed ? aimed.numbers : []}
            winner=${shown && !spinning ? shown.number : null} money=${money}
            onAim=${setAim} onPlace=${place} onCancel=${() => setAim(null)} />`
        : html`<div class="rl-buy-in">
            <p>Buy chips to play. Your stack stays at the table until you cash out.</p>
            <button type="button" disabled=${busy}
              onClick=${() => setBuyInOpen(true)}>
              Choose buy-in
            </button>
          </div>`}
      </div>
    </div>

    <div class="rl-toolbar">
      <div class="rl-tray" role="group" aria-label="Chip value">
        ${table.chips.map(
          (value) => html`<button key=${value} type="button"
            class=${`rl-chip-button ${chip === value ? "on" : ""}`}
            aria-pressed=${chip === value} disabled=${!seated}
            onClick=${() => setChip(value)}>${money(value)}</button>`,
        )}
      </div>
      <div class="rl-actions" role="group" aria-label="Bet controls">
        <button type="button" disabled=${busy || table.staked === 0}
          onClick=${() => act("undo", "/roulette/undo")}>Undo</button>
        <button type="button" disabled=${busy || table.staked === 0}
          onClick=${() => act("clear", "/roulette/clear")}>Clear</button>
        <button type="button" disabled=${busy || !table.last || table.last.chips.length === 0}
          onClick=${() => act("rebet", "/roulette/rebet")}>Rebet</button>
      </div>
    </div>

    ${buyInOpen
      ? html`<${BuyInDialog} amounts=${buyIns} index=${buyInIndex} pending=${busy}
          onIndex=${setBuyInIndex} onCancel=${() => setBuyInOpen(false)} onConfirm=${buyIn} />`
      : null}

    ${error ? html`<p class="error rl-error" role="alert">${error}</p>` : null}
  </div>`;
}

render(html`<${App} />`, root);
