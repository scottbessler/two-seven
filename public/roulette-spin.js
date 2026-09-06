/**
 * Roulette motion, planned before the first frame is drawn.
 *
 * The wheel has to land on a number somebody else picked, and an honest
 * simulation cannot be steered. So the ball's flight is simulated for real and
 * the *rotor's* starting rotation is the free variable: spin it, see which
 * pocket the ball ended up over, then turn the numbers so that pocket carries
 * the number we were told to show. Fret geometry repeats every pocket, so the
 * correction is an exact whole number of pockets and the trajectory it was
 * computed from stays valid -- the ball really does bounce its way into the
 * number it lands on, and nothing about its path depends on the outcome.
 *
 * The result is a timeline sampled at a fixed step. Rendering only reads it, so
 * the motion is identical on a 60Hz phone and a 144Hz monitor, and a dropped
 * frame costs a frame rather than a divergence.
 */

/** A European single-zero wheel, clockwise from the zero. */
export const POCKET_ORDER = [
  0, 32, 15, 19, 4, 21, 2, 25, 17, 34, 6, 27, 13, 36, 11, 30, 8, 23,
  10, 5, 24, 16, 33, 1, 20, 14, 31, 9, 22, 18, 29, 7, 28, 12, 35, 3, 26,
];
export const POCKETS = POCKET_ORDER.length;
/** One pocket, in radians. */
export const STEP = (Math.PI * 2) / POCKETS;

const RED = new Set([1, 3, 5, 7, 9, 12, 14, 16, 18, 19, 21, 23, 25, 27, 30, 32, 34, 36]);

export function pocketColor(number) {
  return number === 0 ? "green" : RED.has(number) ? "red" : "black";
}

export function pocketIndex(number) {
  return POCKET_ORDER.indexOf(number);
}

/**
 * The wheel, in metres. A tournament wheel is 32 inches across, and keeping the
 * real scale is what lets the rest of this file use real gravity: the drop off
 * the track, the arc off a deflector and the height of a hop all come out at
 * casino speed without a single fudge factor for time.
 */
export const WHEEL = {
  rim: 0.4,
  track: 0.372,
  deflector: 0.312,
  numbers: 0.296,
  mouth: 0.262,
  pocket: 0.232,
  cone: 0.15,
  ball: 0.0105,
  fret: 0.012,
};

const G = 9.81;
/** The bowl above the deflectors is nearly a wall; below them it flattens out. */
const UPPER_SLOPE = 1.5;
const LOWER_SLOPE = 0.45;
/** Eight diamonds, drawn where the ball is simulated to strike them. */
export const DEFLECTORS = 8;
const DEFLECTOR_STEP = (Math.PI * 2) / DEFLECTORS;
/** Diamonds straddle the twelve o'clock rather than sitting on it. */
export const DEFLECTOR_OFFSET = DEFLECTOR_STEP / 2;

const DT = 1 / 240;
const MAX_SECONDS = 24;

export const DEFAULT_TUNING = {
  /** How long the ball holds the track before gravity wins, in seconds. */
  trackSeconds: 5.2,
  /** Ball speed at release, in revolutions per second. */
  ballSpeed: 2.4,
  /** Rotor speed, in revolutions per second, against the ball. */
  rotorSpeed: 0.42,
  /** Scales every bounce: 0 is a dead ball, 1 is a casino one. */
  bounce: 1,
};

/** mulberry32 -- small, fast, and the same sequence in every browser. */
export function createRng(seed) {
  let state = seed >>> 0;
  return function next() {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function mod(value, size) {
  return ((value % size) + size) % size;
}

function range(rng, low, high) {
  return low + rng() * (high - low);
}

/** The signed angle equal to `angle`, in (-pi, pi]. */
function wrapPi(angle) {
  return angle - Math.PI * 2 * Math.round(angle / (Math.PI * 2));
}

/**
 * Plan one spin. `number` is the pocket to land on; leave it out and the seed
 * picks one. `rotorAt` is where the rotor is pointing right now, so a wheel
 * that was already turning keeps turning. Everything the renderer needs comes
 * back in the plan, and nothing in it depends on the wall clock.
 */
export function planSpin(options = {}) {
  const tuning = { ...DEFAULT_TUNING, ...options.tuning };
  const seed = (options.seed ?? Math.floor(Math.random() * 2 ** 32)) >>> 0;
  const rng = createRng(seed);
  const number = POCKET_ORDER.includes(options.number)
    ? options.number
    : POCKET_ORDER[Math.floor(rng() * POCKETS)];

  const bounce = Math.max(0, tuning.bounce);
  const rotorSpeed = Math.PI * 2 * tuning.rotorSpeed;
  // The speed below which the track can no longer hold the ball out against
  // gravity. It falls out of the geometry, so a steeper bowl drops it sooner.
  const dropSpeed = Math.sqrt((G * UPPER_SLOPE) / WHEEL.track);
  const releaseSpeed = Math.max(dropSpeed * 1.2, Math.PI * 2 * tuning.ballSpeed);
  const drag = Math.log(releaseSpeed / dropSpeed) / Math.max(0.5, tuning.trackSeconds);

  const angles = [];
  const radii = [];
  const heights = [];
  const impacts = [];

  // The ball runs against the rotor, so its angles only ever decrease.
  let t = 0;
  let angle = 0;
  let radius = WHEEL.track;
  let speed = releaseSpeed;
  let height = 0;
  let rise = 0;
  const record = () => {
    angles.push(angle);
    radii.push(radius);
    heights.push(height);
  };

  // 1. The track. A long, quiet deceleration -- the part that makes the drop
  //    feel earned rather than scripted.
  while (speed > dropSpeed && t < MAX_SECONDS) {
    record();
    speed -= drag * speed * DT;
    angle -= speed * DT;
    t += DT;
  }
  const dropAt = t;

  // 2. The bowl. Angular momentum is what carries the ball round, so as
  //    friction bleeds it away the radius that momentum can hold shrinks and
  //    the ball spirals in -- slowly at first, then all at once.
  let momentum = speed * radius * radius;
  let radial = 0;
  let deflectorArc = Math.floor((angle - DEFLECTOR_OFFSET) / DEFLECTOR_STEP);
  while (radius > WHEEL.mouth && t < MAX_SECONDS) {
    record();
    if (height > 0 || rise > 0) {
      rise -= G * DT;
      height += rise * DT;
      radius += radial * DT;
      if (height <= 0) {
        height = 0;
        rise = -rise * 0.34;
        if (rise < 0.16) rise = 0;
        else impacts.push({ at: t, kind: "bowl", strength: Math.min(1, rise / 0.9) });
      }
    } else {
      const slope = radius > WHEEL.deflector ? UPPER_SLOPE : LOWER_SLOPE;
      const outward = (momentum * momentum) / (radius * radius * radius) - G * slope;
      radial += outward * 0.75 * DT;
      radial -= radial * 2.4 * DT;
      radius += radial * DT;
      momentum -= momentum * 0.42 * DT;
      if (radius > WHEEL.track) {
        radius = WHEEL.track;
        radial = -Math.abs(radial) * 0.4;
      }
    }
    speed = momentum / (radius * radius);
    angle -= speed * DT;
    t += DT;

    // 3. The diamonds. They stand still while the ball comes round, so a strike
    //    is the ball crossing one while it is down at their level -- which is
    //    why no two spins scatter the same way.
    const arc = Math.floor((angle - DEFLECTOR_OFFSET) / DEFLECTOR_STEP);
    const passing = radius < WHEEL.deflector + 0.03 && radius > WHEEL.deflector - 0.025;
    if (arc !== deflectorArc && passing && height < 0.012) {
      const strength = Math.min(1, speed / 8);
      momentum *= range(rng, 0.4, 0.72);
      radial = (rng() < 0.32 ? 1 : -1) * Math.abs(radial || 0.2) * range(rng, 0.3, 0.85);
      rise = range(rng, 0.45, 1.05) * bounce;
      height = 0.0001;
      impacts.push({ at: t, kind: "deflector", strength: Math.max(0.35, strength) });
    }
    deflectorArc = arc;
  }
  const handoffAt = t;

  // 4. The rotor. From here the only thing that matters is the ball's speed
  //    *relative to the pockets*, so the sim moves into the rotor's frame: the
  //    frets stand still and the ball skids across them, clearing them easily
  //    while it is fast and getting thrown about once it is not.
  let relative = angle - rotorSpeed * t;
  let relativeSpeed = -speed - rotorSpeed;
  let pocket = Math.round(relative / STEP);
  // A fret is a wall the ball has to climb; below this it can only rattle.
  const clearSpeed = Math.sqrt(2 * G * WHEEL.fret) / WHEEL.pocket;
  let calm = 0;
  let settleFrom = -1;
  while (t < MAX_SECONDS) {
    record();
    radius += (WHEEL.pocket - radius) * Math.min(1, 7 * DT);
    if (height > 0 || rise > 0) {
      rise -= G * DT;
      height += rise * DT;
      if (height <= 0) {
        height = 0;
        rise = -rise * 0.3;
        if (rise < 0.09) rise = 0;
      }
    }
    if (settleFrom >= 0) {
      // Captured: ease the last fraction of a pocket out so the ball comes to
      // rest against the fret rather than stopping dead in mid-air.
      const eased = Math.min(1, (t - settleFrom) / 0.32);
      relative += (pocket * STEP - relative) * Math.min(1, 9 * DT);
      relativeSpeed = 0;
      if (eased >= 1) {
        relative = pocket * STEP;
        t += DT;
        angle = relative + rotorSpeed * t;
        record();
        break;
      }
      t += DT;
      angle = relative + rotorSpeed * t;
      continue;
    }

    relative += relativeSpeed * DT;
    relativeSpeed -= relativeSpeed * 0.5 * DT;
    const next = Math.round(relative / STEP);
    if (next !== pocket && height <= 0) {
      const direction = Math.sign(next - pocket);
      const wall = (pocket + direction * 0.5) * STEP;
      const fast = Math.min(1, Math.abs(relativeSpeed) / (clearSpeed * 3));
      impacts.push({ at: t, kind: "fret", strength: 0.25 + 0.6 * fast });
      if (Math.abs(relativeSpeed) > clearSpeed && rng() > 0.12 * (1 - fast)) {
        // Over the top: a glancing hit that barely slows a quick ball.
        pocket = next;
        relativeSpeed *= 0.86 + 0.11 * fast;
        rise = range(rng, 0.18, 0.55) * bounce * (0.4 + fast);
        height = 0.0001;
      } else {
        // Not enough left to climb it -- back into the pocket it came from.
        relative = 2 * wall - relative;
        relativeSpeed = -relativeSpeed * range(rng, 0.24, 0.52);
        rise = range(rng, 0.08, 0.22) * bounce;
        height = 0.0001;
      }
    } else if (next !== pocket) {
      pocket = next;
    }
    calm = Math.abs(relativeSpeed) < clearSpeed * 0.2 && height <= 0 ? calm + DT : 0;
    if (calm > 0.12) settleFrom = t;
    t += DT;
    angle = relative + rotorSpeed * t;
  }

  // The one free variable: which number the wheel had under the ball all along.
  const landed = mod(Math.round(relative / STEP), POCKETS);
  const wanted = pocketIndex(number);
  const rotorPhase = (landed - wanted) * STEP;

  // A rotor that was already turning cannot jump to the phase the outcome
  // wants, so it is walked there instead: the whole correction is spent while
  // the ball is still up on the track, easing to nothing by the time it drops.
  // From the drop onwards the rotor turns at exactly the speed the fret
  // simulation above assumed, which is what keeps the landing honest.
  const rotorLead = options.rotorAt === undefined ? 0 : wrapPi(rotorPhase - options.rotorAt);

  return {
    seed,
    number,
    tuning,
    index: wanted,
    rotorPhase,
    rotorLead,
    rotorBlend: dropAt,
    rotorSpeed,
    duration: (angles.length - 1) * DT,
    step: DT,
    angles: Float64Array.from(angles),
    radii: Float64Array.from(radii),
    heights: Float64Array.from(heights),
    impacts,
    marks: { drop: dropAt, handoff: handoffAt, settle: settleFrom < 0 ? t : settleFrom },
    /** Revolutions the ball makes before it drops -- a spin's whole first act. */
    trackRevolutions: (angles[0] - angles[Math.max(0, Math.round(dropAt / DT))]) / (Math.PI * 2),
  };
}

/** Where the rotor is pointing at `time`. It never stops, in life or here. */
export function rotorAngle(plan, time) {
  const t = Math.max(0, time);
  const turned = plan.rotorPhase + plan.rotorSpeed * t;
  if (!plan.rotorLead || t >= plan.rotorBlend) return turned;
  const x = t / plan.rotorBlend;
  return turned - plan.rotorLead * (1 - x * x * (3 - 2 * x));
}

/** The ball at `time`, interpolated. Past the end it rides round in its pocket. */
export function sampleSpin(plan, time) {
  const last = plan.angles.length - 1;
  const at = Math.max(0, time) / plan.step;
  if (at >= last) {
    const drift = plan.rotorSpeed * (Math.max(0, time) - plan.duration);
    return { angle: plan.angles[last] + drift, radius: plan.radii[last], height: 0, resting: true };
  }
  const i = Math.floor(at);
  const f = at - i;
  return {
    angle: plan.angles[i] + (plan.angles[i + 1] - plan.angles[i]) * f,
    radius: plan.radii[i] + (plan.radii[i + 1] - plan.radii[i]) * f,
    height: plan.heights[i] + (plan.heights[i + 1] - plan.heights[i]) * f,
    resting: false,
  };
}

/** The number under the ball at `time` -- the readout the plan promises. */
export function numberUnderBall(plan, time) {
  const { angle } = sampleSpin(plan, time);
  return POCKET_ORDER[mod(Math.round((angle - rotorAngle(plan, time)) / STEP), POCKETS)];
}
