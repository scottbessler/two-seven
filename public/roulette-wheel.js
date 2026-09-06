/**
 * The wheel on screen.
 *
 * Everything about where the ball *is* comes from the plan in roulette-spin.js;
 * this file only decides how it looks. The bowl and the rotor are painted once
 * into offscreen layers and composited each frame -- the rotor rotated, both of
 * them squashed vertically -- so a frame costs two blits and a ball, which is
 * what keeps it smooth on a phone.
 *
 * The squash is the whole trick behind the depth: the wheel is drawn flat, in
 * its own plane, and tipped away from the viewer at composite time. Because the
 * ball's height is added *after* that tip, a hop lifts it clear of the felt and
 * its shadow stays behind on the wood.
 */
import {
  DEFLECTORS,
  DEFLECTOR_OFFSET,
  POCKETS,
  POCKET_ORDER,
  STEP,
  WHEEL,
  planSpin,
  pocketColor,
  rotorAngle,
  sampleSpin,
} from "/public/roulette-spin.js";

const TAU = Math.PI * 2;
/** Canvas measures angles from three o'clock; the wheel measures from twelve. */
const FROM_TWELVE = -Math.PI / 2;

/** Radii the drawing needs that the physics has no opinion about. */
const LIP = 0.386;
const GROOVE = 0.358;
const DIAMOND = 0.32;

const FALLBACK = {
  wood: "#5c3720",
  woodLight: "#8a5533",
  woodDark: "#25150c",
  groove: "#1b0f08",
  bowl: "#4a2c19",
  bowlDark: "#1f1209",
  metal: "#ccd4d8",
  metalDark: "#5d686e",
  red: "#b32219",
  black: "#191b19",
  green: "#0f6b45",
  ink: "#f4efe4",
  cone: "#b6bec3",
  coneDark: "#3f484d",
  gold: "#d9ad55",
  glow: "#f1d56e",
  ball: "#f8f6ef",
  ballDark: "#a8a79d",
};

function palette(element) {
  const style = getComputedStyle(element);
  const read = (name) => style.getPropertyValue(`--wheel-${name}`).trim();
  const colors = {};
  for (const key of Object.keys(FALLBACK)) {
    colors[key] = read(key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)) || FALLBACK[key];
  }
  colors.font = style.getPropertyValue("--font-ui").trim() || "serif";
  return colors;
}

function circle(ctx, radius) {
  ctx.beginPath();
  ctx.arc(0, 0, radius, 0, TAU);
}

function ring(ctx, inner, outer) {
  ctx.beginPath();
  ctx.arc(0, 0, outer, 0, TAU);
  ctx.arc(0, 0, inner, 0, TAU, true);
  ctx.closePath();
}

/** A radial gradient lit from the upper left, which is where every light is. */
function lit(ctx, inner, outer, near, far) {
  const gradient = ctx.createRadialGradient(-outer * 0.3, -outer * 0.4, inner * 0.3, 0, 0, outer * 1.1);
  gradient.addColorStop(0, near);
  gradient.addColorStop(1, far);
  return gradient;
}

function paintBowl(ctx, unit, colors) {
  const rim = WHEEL.rim * unit;
  const lip = LIP * unit;
  const groove = GROOVE * unit;
  const deflector = WHEEL.deflector * unit;
  const mouth = WHEEL.numbers * unit;

  // The turned wood body. One gradient carries the whole curve of the bowl,
  // from the lit outer edge down into the shade at the back of the well.
  circle(ctx, rim);
  ctx.fillStyle = lit(ctx, 0, rim, colors.woodLight, colors.woodDark);
  ctx.fill();

  // Polished wood catches the light in bands as it turns. Four of them, faint
  // enough to read as sheen rather than stripes.
  if (typeof ctx.createConicGradient === "function") {
    const sheen = ctx.createConicGradient(FROM_TWELVE, 0, 0);
    for (let i = 0; i <= 8; i++) {
      sheen.addColorStop(i / 8, i % 2 ? "rgba(255,236,205,.10)" : "rgba(0,0,0,.10)");
    }
    ring(ctx, lip * 0.98, rim);
    ctx.fillStyle = sheen;
    ctx.fill();
  }

  // The lip the ball is thrown against, and the groove it runs in: the groove
  // floor is the darkest thing on the wheel, so a white ball reads at any speed.
  ring(ctx, lip, rim);
  ctx.strokeStyle = "rgba(255,236,205,.22)";
  ctx.lineWidth = Math.max(1, unit * 0.003);
  ctx.stroke();
  ring(ctx, groove, lip);
  const track = ctx.createRadialGradient(0, 0, groove, 0, 0, lip);
  track.addColorStop(0, colors.groove);
  track.addColorStop(0.7, colors.groove);
  track.addColorStop(1, colors.wood);
  ctx.fillStyle = track;
  ctx.fill();

  // The steep upper bowl, then the shallow apron the ball crosses on its way in.
  ring(ctx, deflector, groove);
  ctx.fillStyle = lit(ctx, deflector, groove, colors.woodLight, colors.bowlDark);
  ctx.fill();
  ring(ctx, mouth, deflector);
  ctx.fillStyle = lit(ctx, mouth, deflector, colors.bowl, colors.bowlDark);
  ctx.fill();

  for (let i = 0; i < DEFLECTORS; i++) {
    ctx.save();
    ctx.rotate(DEFLECTOR_OFFSET + (i * TAU) / DEFLECTORS);
    ctx.translate(0, -DIAMOND * unit);
    // Half of them stand along the radius and half across it, so the ball is
    // thrown a different way depending on which one it finds.
    const long = 0.03 * unit;
    const short = 0.0165 * unit;
    const [wide, tall] = i % 2 ? [short, long] : [long, short];
    ctx.beginPath();
    ctx.moveTo(0, -tall);
    ctx.lineTo(wide, 0);
    ctx.lineTo(0, tall);
    ctx.lineTo(-wide, 0);
    ctx.closePath();
    const face = ctx.createLinearGradient(-wide, -tall, wide, tall);
    face.addColorStop(0, colors.metal);
    face.addColorStop(0.5, colors.metalDark);
    face.addColorStop(1, colors.metal);
    ctx.fillStyle = face;
    ctx.fill();
    ctx.strokeStyle = "rgba(0,0,0,.45)";
    ctx.lineWidth = Math.max(1, unit * 0.0015);
    ctx.stroke();
    ctx.restore();
  }

  // The well the rotor drops into: a hard shadow at its edge sells the depth.
  ring(ctx, mouth * 0.985, mouth);
  ctx.fillStyle = "rgba(0,0,0,.55)";
  ctx.fill();

  // The wheel's own silhouette, so its edge does not fray into the felt.
  circle(ctx, rim * 0.998);
  ctx.strokeStyle = "rgba(0,0,0,.5)";
  ctx.lineWidth = Math.max(1.5, unit * 0.006);
  ctx.stroke();
}

function paintRotor(ctx, unit, colors) {
  const outer = WHEEL.numbers * unit;
  const mouth = WHEEL.mouth * unit;
  const cone = WHEEL.cone * unit;

  for (let i = 0; i < POCKETS; i++) {
    const number = POCKET_ORDER[i];
    ctx.beginPath();
    ctx.moveTo(0, 0);
    ctx.arc(0, 0, outer, (i - 0.5) * STEP + FROM_TWELVE, (i + 0.5) * STEP + FROM_TWELVE);
    ctx.closePath();
    ctx.fillStyle = colors[pocketColor(number)];
    ctx.fill();
  }

  // The pockets sit below the number ring and in its shadow -- one overlay
  // does that for all thirty-seven without darkening each colour by hand.
  ring(ctx, cone * 0.98, mouth);
  const well = ctx.createRadialGradient(0, 0, cone * 0.98, 0, 0, mouth);
  well.addColorStop(0, "rgba(0,0,0,.66)");
  well.addColorStop(0.75, "rgba(0,0,0,.42)");
  well.addColorStop(1, "rgba(0,0,0,.12)");
  ctx.fillStyle = well;
  ctx.fill();

  // Frets: a polished blade standing between every pair of pockets, throwing a
  // shadow into the pocket beside it. They are what the ball rattles off at the
  // end of a spin, so they are worth drawing as metal rather than as a line.
  const blade = Math.max(1.4, unit * 0.004);
  const depth = mouth - cone * 0.98;
  const face = ctx.createLinearGradient(0, -mouth, 0, -cone);
  face.addColorStop(0, colors.metal);
  face.addColorStop(0.55, colors.metalDark);
  face.addColorStop(1, "#8b959a");
  for (let i = 0; i < POCKETS; i++) {
    ctx.save();
    ctx.rotate((i + 0.5) * STEP);
    ctx.fillStyle = "rgba(0,0,0,.5)";
    ctx.fillRect(-blade * 0.1, -mouth, blade * 1.1, depth);
    ctx.fillStyle = face;
    ctx.fillRect(-blade * 0.6, -mouth, blade, depth);
    ctx.fillStyle = "rgba(255,255,255,.45)";
    ctx.fillRect(-blade * 0.6, -mouth, blade * 0.3, depth);
    ctx.restore();
  }

  // The ring the numbers are painted on, fenced off from the pockets.
  ring(ctx, mouth, outer);
  ctx.strokeStyle = colors.metalDark;
  ctx.lineWidth = Math.max(1, unit * 0.004);
  ctx.stroke();
  ctx.font = `700 ${Math.max(7, unit * 0.024).toFixed(1)}px ${colors.font}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillStyle = colors.ink;
  for (let i = 0; i < POCKETS; i++) {
    ctx.save();
    ctx.rotate(i * STEP);
    // Numbers read from outside the wheel, so they turn with it.
    ctx.fillText(String(POCKET_ORDER[i]), 0, -(mouth + outer) / 2);
    ctx.restore();
  }

  // The cone the ball rolls off, and the spindle standing on top of it. Turned
  // metal is mostly dark with one hard highlight, so the gradient stays tight
  // rather than washing the middle of the wheel out.
  circle(ctx, cone);
  const dome = ctx.createRadialGradient(-cone * 0.45, -cone * 0.52, cone * 0.04, 0, 0, cone * 1.02);
  dome.addColorStop(0, colors.cone);
  dome.addColorStop(0.42, "#8d979d");
  dome.addColorStop(1, colors.coneDark);
  ctx.fillStyle = dome;
  ctx.fill();
  // Turned metal, not a pearl: faint concentric steps break the dome up and
  // stop the middle of the wheel outshining the numbers round it.
  for (let i = 1; i <= 4; i++) {
    circle(ctx, cone * (i / 5));
    ctx.strokeStyle = i % 2 ? "rgba(255,255,255,.09)" : "rgba(0,0,0,.16)";
    ctx.lineWidth = Math.max(1, unit * 0.0035);
    ctx.stroke();
  }
  circle(ctx, cone);
  ctx.strokeStyle = "rgba(0,0,0,.55)";
  ctx.lineWidth = Math.max(1, unit * 0.005);
  ctx.stroke();
  for (let i = 0; i < 4; i++) {
    ctx.save();
    ctx.rotate((i * TAU) / 4);
    ctx.fillStyle = colors.gold;
    ctx.fillRect(-cone * 0.022, -cone * 0.62, cone * 0.044, cone * 0.42);
    ctx.restore();
  }
  circle(ctx, cone * 0.24);
  ctx.fillStyle = lit(ctx, 0, cone * 0.24, colors.glow, colors.gold);
  ctx.fill();
  circle(ctx, cone * 0.07);
  ctx.fillStyle = "rgba(0,0,0,.6)";
  ctx.fill();
}

function layer(side, dpr) {
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(side * dpr));
  canvas.height = canvas.width;
  const ctx = canvas.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, (side * dpr) / 2, (side * dpr) / 2);
  return { canvas, ctx };
}

/**
 * Mount a wheel on a canvas. `spin` plans and runs one; `seek` freezes it at a
 * given second, which is how the tests look at a moving thing.
 */
export function createWheel(canvas, options = {}) {
  const ctx = canvas.getContext("2d");
  const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)");
  let settings = { tilt: 0.8, trail: 1, tuning: {}, ...options };
  let colors = palette(canvas);
  let dpr = 1;
  let width = 0;
  let height = 0;
  let unit = 0;
  let bowl = null;
  let rotor = null;
  let rotorSide = 0;
  let plan = null;
  let startedAt = 0;
  let announced = false;
  let struck = 0;
  let idlePhase = 0;
  let idleAt = performance.now();
  let frozen = null;
  let raf = 0;

  function idleSpeed() {
    return TAU * (settings.tuning.rotorSpeed ?? 0.42);
  }

  function elapsed(now) {
    return plan ? (now - startedAt) / 1000 : 0;
  }

  function rotorNow(now) {
    // A wheel that turns forever is right for a casino and wrong for anyone who
    // asked the browser to hold still.
    if (reduced?.matches) return plan ? rotorAngle(plan, plan.duration) : idlePhase;
    if (plan) return rotorAngle(plan, elapsed(now));
    return idlePhase + (idleSpeed() * (now - idleAt)) / 1000;
  }

  function build() {
    const box = canvas.getBoundingClientRect();
    // A hidden canvas measures zero, and a wheel drawn at zero radius is a
    // divide by nothing. The observer calls back when it is on screen again.
    if (box.width < 2 || box.height < 2) return;
    dpr = Math.min(3, window.devicePixelRatio || 1);
    width = Math.max(1, Math.round(box.width));
    height = Math.max(1, Math.round(box.height));
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    colors = palette(canvas);
    // The wheel is squashed on screen, so it is the squashed height that has to
    // fit -- otherwise the bowl grows a flat top and bottom on a short page.
    const wheelRadius = Math.min(width / 2, height / (2 * settings.tilt)) * 0.93;
    unit = wheelRadius / WHEEL.rim;
    const bowlLayer = layer(wheelRadius * 2, dpr);
    paintBowl(bowlLayer.ctx, unit, colors);
    bowl = bowlLayer.canvas;
    rotorSide = WHEEL.numbers * unit * 2;
    const rotorLayer = layer(rotorSide, dpr);
    paintRotor(rotorLayer.ctx, unit, colors);
    rotor = rotorLayer.canvas;
  }

  /** How fast the ball is actually travelling, in metres per second. */
  function ballSpeed(time) {
    const a = sampleSpin(plan, time);
    const b = sampleSpin(plan, time + 0.01);
    return (Math.abs(b.angle - a.angle) / 0.01) * a.radius;
  }

  function ballAt(time) {
    const sample = sampleSpin(plan, time);
    const r = sample.radius * unit;
    const surface = -r * Math.cos(sample.angle) * settings.tilt;
    return {
      x: r * Math.sin(sample.angle),
      y: surface - sample.height * unit * 0.95,
      surface,
      height: sample.height,
    };
  }

  function draw(now) {
    const time = elapsed(now);
    const wheelRadius = WHEEL.rim * unit;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    ctx.save();
    ctx.translate(width / 2, height / 2);

    // The wheel's own shadow on the felt, thrown a little below it.
    ctx.save();
    ctx.scale(1, settings.tilt);
    ctx.translate(0, wheelRadius * 0.06);
    const cast = ctx.createRadialGradient(0, 0, wheelRadius * 0.82, 0, 0, wheelRadius * 1.06);
    cast.addColorStop(0, "rgba(0,0,0,.5)");
    cast.addColorStop(1, "rgba(0,0,0,0)");
    circle(ctx, wheelRadius * 1.06);
    ctx.fillStyle = cast;
    ctx.fill();
    ctx.restore();

    ctx.save();
    ctx.scale(1, settings.tilt);
    ctx.drawImage(bowl, -wheelRadius, -wheelRadius, wheelRadius * 2, wheelRadius * 2);
    ctx.rotate(rotorNow(now));
    ctx.drawImage(rotor, -rotorSide / 2, -rotorSide / 2, rotorSide, rotorSide);
    ctx.restore();

    if (plan && time >= plan.duration) {
      // The winner, lit from underneath. It pulses because a still wheel with a
      // still highlight looks like a screenshot.
      const pulse = 0.55 + 0.45 * Math.sin(time * 5);
      ctx.save();
      ctx.scale(1, settings.tilt);
      ctx.rotate(rotorNow(now) + plan.index * STEP);
      ctx.beginPath();
      ctx.arc(0, 0, WHEEL.numbers * unit, -STEP / 2 + FROM_TWELVE, STEP / 2 + FROM_TWELVE);
      ctx.arc(0, 0, WHEEL.cone * unit, STEP / 2 + FROM_TWELVE, -STEP / 2 + FROM_TWELVE, true);
      ctx.closePath();
      ctx.globalCompositeOperation = "lighter";
      ctx.fillStyle = colors.glow;
      ctx.globalAlpha = 0.1 + 0.08 * pulse;
      ctx.fill();
      ctx.globalAlpha = 0.35 + 0.35 * pulse;
      ctx.strokeStyle = colors.glow;
      ctx.lineWidth = Math.max(1.5, unit * 0.005);
      ctx.stroke();
      ctx.restore();
    }

    if (plan) {
      const ball = ballAt(time);
      const size = Math.max(3.2, WHEEL.ball * unit);

      // A ball crossing the screen at four metres a second covers its own width
      // twice between frames, which strobes. Stroking the path it actually took
      // over the last fifty milliseconds turns that back into a streak -- and
      // because the streak is the real path, it shortens to nothing on its own
      // as the ball slows, with no threshold to tune.
      const steps = 10;
      const span = 0.05 * settings.trail;
      if (span > 0) {
        ctx.lineCap = "round";
        ctx.strokeStyle = colors.ball;
        let from = ball;
        for (let i = 1; i <= steps; i++) {
          const to = ballAt(Math.max(0, time - (i * span) / steps));
          const fade = 1 - (i - 1) / steps;
          ctx.globalAlpha = 0.3 * fade * fade;
          ctx.lineWidth = size * 1.7 * fade;
          ctx.beginPath();
          ctx.moveTo(from.x, from.y);
          ctx.lineTo(to.x, to.y);
          ctx.stroke();
          from = to;
        }
        ctx.globalAlpha = 1;
      }

      // The shadow stays on the wood while the ball is in the air, which is the
      // only cue in a flat drawing that says how high a hop went.
      const lift = Math.min(1, ball.height / 0.05);
      ctx.beginPath();
      ctx.ellipse(ball.x, ball.surface, size * (1 + lift * 1.1), size * settings.tilt * (1 + lift), 0, 0, TAU);
      ctx.fillStyle = `rgba(0,0,0,${(0.45 * (1 - lift * 0.7)).toFixed(3)})`;
      ctx.fill();

      ctx.beginPath();
      ctx.arc(ball.x, ball.y, size, 0, TAU);
      const shine = ctx.createRadialGradient(
        ball.x - size * 0.4, ball.y - size * 0.45, size * 0.1,
        ball.x, ball.y, size,
      );
      shine.addColorStop(0, "#fff");
      shine.addColorStop(0.45, colors.ball);
      shine.addColorStop(1, colors.ballDark);
      ctx.fillStyle = shine;
      ctx.fill();
    }
    ctx.restore();
  }

  function pump(now) {
    raf = requestAnimationFrame(pump);
    if (plan) {
      const time = elapsed(now);
      while (struck < plan.impacts.length && plan.impacts[struck].at <= time) {
        settings.onImpact?.(plan.impacts[struck]);
        struck++;
      }
      if (!announced && time >= plan.duration) {
        announced = true;
        settings.onResult?.(plan);
      }
      settings.onFrame?.({ time, speed: ballSpeed(time), resting: time >= plan.duration });
    }
    draw(now);
    if (reduced?.matches && (!plan || announced)) stop();
  }

  function start() {
    if (!raf) raf = requestAnimationFrame(pump);
  }

  function stop() {
    cancelAnimationFrame(raf);
    raf = 0;
  }

  build();
  start();

  const observer = typeof ResizeObserver === "function" ? new ResizeObserver(build) : null;
  observer?.observe(canvas);
  // Numbers painted before Bitter arrives are painted in a fallback face.
  document.fonts?.ready.then(build).catch(() => {});

  return {
    spin(request = {}) {
      const now = performance.now();
      plan = planSpin({ ...request, tuning: settings.tuning, rotorAt: rotorNow(now) });
      startedAt = now;
      announced = false;
      struck = 0;
      if (reduced?.matches) {
        // No orbit for anyone who asked not to be spun: the wheel arrives at
        // the answer, and the answer is the part that mattered.
        startedAt = now - plan.duration * 1000;
        struck = plan.impacts.length;
      }
      frozen = null;
      start();
      return plan;
    },
    /** Freeze on one frame. Playwright cannot watch, so it looks instead. */
    seek(seconds) {
      if (!plan) return;
      stop();
      frozen = seconds;
      startedAt = performance.now() - seconds * 1000;
      draw(performance.now());
    },
    resume() {
      if (frozen != null && plan) startedAt = performance.now() - frozen * 1000;
      frozen = null;
      start();
    },
    setTuning(tuning) {
      const now = performance.now();
      idlePhase = rotorNow(now);
      idleAt = now;
      settings = { ...settings, tuning: { ...settings.tuning, ...tuning } };
    },
    setView(view) {
      settings = { ...settings, ...view };
      build();
      if (frozen != null) draw(performance.now());
    },
    plan: () => plan,
    time: () => (plan ? elapsed(performance.now()) : 0),
    destroy() {
      stop();
      observer?.disconnect();
    },
  };
}
