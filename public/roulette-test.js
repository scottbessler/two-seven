/**
 * The wheel on a page of its own, with every number the motion depends on
 * exposed as a slider.
 *
 * There is no betting here and no server: this page exists to answer one
 * question -- is the spin worth building a game on -- and the fastest way to
 * answer it is to spin it a hundred times, at every setting, and watch. The
 * "land on" picker is the other half of that: a spin that has to end on a
 * number you chose is a spin you can catch cheating.
 */
import { html, render, useEffect, useRef, useState } from "/public/vendor/htm-preact.js";
import { DEFAULT_TUNING, POCKET_ORDER, planSpin, pocketColor, numberUnderBall } from "/public/roulette-spin.js";
import { createRouletteSound } from "/public/roulette-sound.js";
import { createWheel } from "/public/roulette-wheel.js";

const MOTION = [
  { key: "trackSeconds", label: "Time on track", min: 2, max: 9, step: 0.1, unit: "s" },
  { key: "ballSpeed", label: "Ball speed", min: 1.2, max: 3.6, step: 0.05, unit: " rev/s" },
  { key: "rotorSpeed", label: "Wheel speed", min: 0.15, max: 0.9, step: 0.01, unit: " rev/s" },
  { key: "bounce", label: "Bounce", min: 0, max: 1.6, step: 0.05, unit: "×" },
];
const VIEW = [
  { key: "tilt", label: "Tilt", min: 0.45, max: 1, step: 0.01, unit: "" },
  { key: "trail", label: "Ball trail", min: 0, max: 1.5, step: 0.05, unit: "×" },
];
const NUMBERS = POCKET_ORDER.toSorted((a, b) => a - b);

function Chip({ number, size = "" }) {
  return html`<span class=${`roulette-chip ${pocketColor(number)} ${size}`}>${number}</span>`;
}

function Slider({ spec, value, onInput }) {
  return html`<label class="roulette-slider">
    <span>${spec.label}<b>${value.toFixed(spec.step < 0.1 ? 2 : 1)}${spec.unit}</b></span>
    <input type="range" min=${spec.min} max=${spec.max} step=${spec.step} value=${value}
      onInput=${(event) => onInput(spec.key, Number(event.target.value))} />
  </label>`;
}

function App() {
  const frame = useRef(null);
  const wheel = useRef(null);
  const sound = useRef(null);
  const live = useRef({});
  const [tuning, setTuning] = useState({ ...DEFAULT_TUNING });
  const [view, setView] = useState({ tilt: 0.8, trail: 1 });
  const [target, setTarget] = useState("random");
  const [spinning, setSpinning] = useState(false);
  const [result, setResult] = useState(null);
  const [history, setHistory] = useState([]);
  const [audible, setAudible] = useState(true);
  const [audit, setAudit] = useState(null);

  live.current = { audible };

  useEffect(() => {
    sound.current = createRouletteSound();
    wheel.current = createWheel(frame.current, {
      ...view,
      tuning,
      onImpact: (impact) => sound.current.strike(impact.kind, impact.strength),
      onFrame: ({ speed, resting }) => sound.current.roll(resting ? 0 : speed),
      onResult: (plan) => {
        sound.current.stop();
        setSpinning(false);
        setResult({
          number: plan.number,
          landed: numberUnderBall(plan, plan.duration),
          seconds: plan.duration,
          seed: plan.seed,
          revolutions: plan.trackRevolutions,
          bounces: plan.impacts.length,
        });
        setHistory((past) => [plan.number, ...past].slice(0, 16));
      },
    });
    // Test hook: the e2e run drives the wheel through this rather than clicks,
    // so it can hold a moving thing still and look at it.
    window.rouletteWheel = wheel.current;
    return () => {
      wheel.current.destroy();
      delete window.rouletteWheel;
    };
  }, []);

  useEffect(() => {
    wheel.current?.setTuning(tuning);
  }, [tuning]);
  useEffect(() => {
    wheel.current?.setView(view);
  }, [view]);
  useEffect(() => {
    sound.current?.setMuted(!audible);
  }, [audible]);

  const spin = () => {
    if (spinning) return;
    setSpinning(true);
    setResult(null);
    if (live.current.audible) sound.current.begin();
    wheel.current.spin({ number: target === "random" ? undefined : Number(target) });
  };

  // Thirty-seven numbers, eight seeds apiece: if the plan can be steered, this
  // is where it says so.
  const check = () => {
    setAudit("running");
    setTimeout(() => {
      let missed = 0;
      let seconds = 0;
      for (const number of NUMBERS) {
        for (let seed = 1; seed <= 8; seed++) {
          const plan = planSpin({ number, seed: seed * 7919, tuning, rotorAt: seed });
          if (numberUnderBall(plan, plan.duration) !== number) missed++;
          seconds += plan.duration;
        }
      }
      const spins = NUMBERS.length * 8;
      setAudit({ spins, missed, average: seconds / spins });
    }, 20);
  };

  return html`<div class="roulette-stage">
    <canvas class="roulette-canvas" ref=${frame} aria-label="Roulette wheel"></canvas>
    <div class="roulette-readout" role="status">
      ${result
        ? html`<${Chip} number=${result.number} size="big" />
            <div class="roulette-facts">
              <span>${result.seconds.toFixed(1)}s · ${result.revolutions.toFixed(1)} revs · ${result.bounces} bounces</span>
              <span class=${result.landed === result.number ? "roulette-ok" : "roulette-bad"}>
                ${result.landed === result.number ? `landed on ${result.landed} as asked` : `asked for ${result.number}, landed on ${result.landed}`}
              </span>
            </div>`
        : html`<span class="roulette-waiting">${spinning ? "No more bets" : "Spin the wheel"}</span>`}
    </div>
    <div class="roulette-controls">
      <button class="roulette-spin" type="button" onClick=${spin} disabled=${spinning}>
        ${spinning ? "Spinning…" : "Spin"}
      </button>
      <label class="roulette-target">Land on
        <select value=${target} onChange=${(event) => setTarget(event.target.value)}>
          <option value="random">Any number</option>
          ${NUMBERS.map((number) => html`<option value=${number}>${number}</option>`)}
        </select>
      </label>
      <button class=${`roulette-mute ${audible ? "" : "off"}`} type="button" aria-pressed=${audible}
        onClick=${() => setAudible((on) => !on)}>${audible ? "Sound on" : "Sound off"}</button>
    </div>
    <div class="roulette-history">
      ${history.length === 0
        ? html`<span class="roulette-waiting">Previous numbers appear here</span>`
        : history.map((number, index) => html`<${Chip} key=${index} number=${number} />`)}
    </div>
    <details class="roulette-tuning">
      <summary>Tune the spin</summary>
      <div class="roulette-knobs">
        ${MOTION.map((spec) => html`<${Slider} key=${spec.key} spec=${spec} value=${tuning[spec.key]}
          onInput=${(key, value) => setTuning((was) => ({ ...was, [key]: value }))} />`)}
        ${VIEW.map((spec) => html`<${Slider} key=${spec.key} spec=${spec} value=${view[spec.key]}
          onInput=${(key, value) => setView((was) => ({ ...was, [key]: value }))} />`)}
      </div>
      <div class="roulette-audit">
        <button type="button" onClick=${check} disabled=${audit === "running"}>Check 296 spins</button>
        ${audit === "running" ? html`<span>Spinning them all…</span>` : null}
        ${audit && audit !== "running"
          ? html`<span class=${audit.missed ? "roulette-bad" : "roulette-ok"}>
              ${audit.missed ? `${audit.missed} of ${audit.spins} missed` : `all ${audit.spins} landed on the number asked for`}
              · ${audit.average.toFixed(1)}s each
            </span>`
          : null}
      </div>
    </details>
  </div>`;
}

render(html`<${App} />`, document.getElementById("roulette-app"));
