import { expect, test } from "./fixtures";
import { expectLayout } from "./layout";

const LAYOUT = [
  ".roulette-stage",
  ".roulette-canvas",
  ".roulette-readout",
  ".roulette-controls",
  ".roulette-spin",
  ".roulette-target",
  ".roulette-history",
];

/** The page hands the wheel to the tests, because a spin cannot be hurried. */
declare global {
  interface Window {
    rouletteWheel: {
      spin(request: { number?: number; seed?: number }): {
        number: number;
        duration: number;
        rotorSpeed: number;
        marks: { drop: number; handoff: number; settle: number };
        impacts: { at: number; kind: string }[];
      };
      seek(seconds: number): void;
      plan(): { number: number; duration: number } | null;
    };
  }
}

test.describe("roulette wheel", () => {
  test("lands on the number it was told to, from every pocket", async ({ page }) => {
    await page.goto("/roulette-test");
    await page.waitForFunction(() => Boolean(window.rouletteWheel));
    // The whole design rests on this: the ball's flight is simulated without
    // reference to the outcome, and the rotor is turned to meet it. If that is
    // ever wrong it is wrong for particular numbers and particular bounces, so
    // the check is every pocket against several scatters -- and against a rotor
    // that was already turning, which is the case with a correction to make.
    const misses = await page.evaluate(async () => {
      const { POCKET_ORDER, planSpin, numberUnderBall } = await import("/public/roulette-spin.js");
      const bad: string[] = [];
      for (const number of POCKET_ORDER) {
        for (let seed = 1; seed <= 6; seed++) {
          const plan = planSpin({ number, seed: seed * 104729, rotorAt: seed * 0.911 });
          const landed = numberUnderBall(plan, plan.duration + 5);
          if (landed !== number) bad.push(`asked ${number}, landed ${landed} (seed ${seed})`);
        }
      }
      return bad;
    });
    expect(misses).toEqual([]);
  });

  test("keeps the rotor turning across spins and settles the ball in a pocket", async ({ page }) => {
    await page.goto("/roulette-test");
    await page.waitForFunction(() => Boolean(window.rouletteWheel));
    const spin = await page.evaluate(async () => {
      const { STEP, planSpin, rotorAngle, sampleSpin } = await import("/public/roulette-spin.js");
      // A wheel that is already turning must not jump when the next outcome is
      // planned: the correction is spent while the ball is still on the track.
      const turning = 2.4;
      const plan = planSpin({ number: 26, seed: 99, rotorAt: turning });
      const rest = sampleSpin(plan, plan.duration);
      const offset = (rest.angle - rotorAngle(plan, plan.duration)) / STEP;
      return {
        jump: Math.abs(
          (rotorAngle(plan, 0) - turning) - Math.PI * 2 * Math.round((rotorAngle(plan, 0) - turning) / (Math.PI * 2)),
        ),
        // How far the resting ball sits from the middle of its pocket.
        fromCentre: Math.abs(offset - Math.round(offset)),
        radius: rest.radius,
        // Nothing is left in the air at the end of a spin.
        height: rest.height,
      };
    });
    expect(spin.jump).toBeLessThan(1e-9);
    expect(spin.fromCentre, "the ball rests in the middle of a pocket").toBeLessThan(0.02);
    expect(spin.radius).toBeCloseTo(0.232, 3);
    expect(spin.height).toBe(0);
  });

  test("bounces: off the diamonds on the way down, off the frets on the way in", async ({ page }) => {
    await page.goto("/roulette-test");
    await page.waitForFunction(() => Boolean(window.rouletteWheel));
    const shape = await page.evaluate(() => {
      const plan = window.rouletteWheel.spin({ number: 17, seed: 42 });
      window.rouletteWheel.seek(0);
      const kinds = plan.impacts.map((impact) => impact.kind);
      return {
        seconds: plan.duration,
        onTrack: plan.marks.drop,
        deflectors: kinds.filter((kind) => kind === "deflector").length,
        frets: kinds.filter((kind) => kind === "fret").length,
        // Every impact happens after the ball has left the track.
        early: plan.impacts.filter((impact) => impact.at < plan.marks.drop).length,
      };
    });
    // A spin has three acts, and none of them may collapse: a long orbit, a
    // scatter off the diamonds, and a rattle across the frets.
    expect(shape.seconds).toBeGreaterThan(6);
    expect(shape.seconds).toBeLessThan(12);
    expect(shape.onTrack).toBeGreaterThan(3);
    expect(shape.deflectors).toBeGreaterThanOrEqual(1);
    expect(shape.frets).toBeGreaterThanOrEqual(3);
    expect(shape.early).toBe(0);
  });

  test("spinning the wheel announces the number it stopped on", async ({ page }) => {
    const problems: string[] = [];
    page.on("pageerror", (error) => problems.push(String(error)));
    await page.goto("/roulette-test");
    await page.waitForFunction(() => Boolean(window.rouletteWheel));
    await page.getByLabel("Land on").selectOption("11");
    await page.getByRole("button", { name: "Spin" }).click();
    await expect(page.getByRole("button", { name: "Spinning…" })).toBeVisible();
    // Mid-flight the ball is somewhere it was not a moment ago.
    const moved = await page.evaluate(async () => {
      const { sampleSpin } = await import("/public/roulette-spin.js");
      const plan = window.rouletteWheel.plan();
      return Math.abs(sampleSpin(plan, 0).angle - sampleSpin(plan, 1).angle);
    });
    expect(moved, "the ball covers ground in its first second").toBeGreaterThan(Math.PI * 2);
    await expect(page.locator(".roulette-chip.big")).toHaveText("11", { timeout: 20_000 });
    await expect(page.locator(".roulette-ok")).toContainText("landed on 11 as asked");
    await expect(page.locator(".roulette-history .roulette-chip")).toHaveText(["11"]);
    expect(problems).toEqual([]);
  });

  test("goes straight to the answer when motion is not wanted", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/roulette-test");
    await page.waitForFunction(() => Boolean(window.rouletteWheel));
    await page.getByLabel("Land on").selectOption("0");
    await page.getByRole("button", { name: "Spin" }).click();
    // No orbit to sit through: the result is there before a spin would have
    // finished its first revolution.
    await expect(page.locator(".roulette-chip.big")).toHaveText("0", { timeout: 2000 });
    await expect(page.locator(".roulette-chip.big")).toHaveClass(/green/);
  });

  test("holds its host-stable layout (V74)", async ({ page }) => {
    await page.goto("/roulette-test");
    await page.waitForFunction(() => Boolean(window.rouletteWheel));
    await expectLayout(page, "roulette-page", LAYOUT);
  });
});

let player = 0;
async function openRoulette(page) {
  player += 1;
  await page.goto("/");
  const suffix = `${Date.now()}${player}${Math.random().toString(36).slice(2, 7)}`;
  await page.fill('#register-form input[name="username"]', `roulette${suffix}`);
  await page.click("#register-form button");
  await page.waitForTimeout(300);
  // A fresh account has nothing; the re-up is where a player's first money
  // comes from, and the buy-in is exactly what it gives.
  await page.evaluate(() =>
    fetch("/api/bank", {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: "{}",
    }),
  );
  await page.goto("/roulette");
}

async function sitDown(page) {
  await openRoulette(page);
  await page.getByRole("button", { name: "Choose buy-in" }).click();
  await page.getByRole("button", { name: "Buy in for $1,000.00" }).click();
  await expect(page.locator(".rl-board")).toBeVisible();
}

/** The box of a numbered square, which is where every inside bet is aimed. */
async function square(page, n: number) {
  const box = await page.locator(`[data-cell="${Math.floor((n - 1) / 3)},${(n - 1) % 3}"]`).boundingBox();
  if (!box) throw new Error(`no square for ${n}`);
  return box;
}

/**
 * Press at a fraction of a square and lift, which is how a chip is placed.
 *
 * The fractions are the *board's* — across the columns and down the rows of the
 * layout as the felt describes it. A wide screen lays that board out a quarter
 * turn round, so the press is turned with it. Every test below therefore names
 * one bet and asserts it in both orientations, which is the property that
 * matters: the same place on the felt is the same bet whichever way it is hung.
 */
async function drop(page, n: number, fx: number, fy: number) {
  const box = await square(page, n);
  const across = (await page.locator(".rl-board.across").count()) > 0;
  const [sx, sy] = across ? [fy, 1 - fx] : [fx, fy];
  await page.mouse.move(box.x + box.width * sx, box.y + box.height * sy);
  const aim = (await page.locator(".rl-aim").innerText()).replaceAll("\n", " ");
  await page.mouse.down();
  await page.mouse.up();
  await page.waitForTimeout(120);
  return aim;
}

test.describe("roulette table", () => {
  test("the buy-in dialog offers the logarithmic stack ladder", async ({ page }) => {
    await openRoulette(page);
    await page.getByRole("button", { name: "Choose buy-in" }).click();
    const dialog = page.getByRole("dialog", { name: "Choose your stack" });
    await expect(dialog).toBeVisible();
    await expect(dialog.locator(".rl-buy-in-rungs span")).toHaveText([
      "$1,000", "$10,000", "$100,000", "$1,000,000",
    ]);
    await dialog.getByRole("slider").fill("1");
    await expect(dialog.locator("output")).toHaveText("$10,000.00");
    await expect(dialog.getByRole("button", { name: "Buy in for $10,000.00" })).toBeVisible();
  });

  test("the board can only name bets the server will price", async ({ page }) => {
    await sitDown(page);
    // The felt's geometry and the server's catalogue are two descriptions of one
    // board. This walks every zone of every square and checks that the ids the
    // first produces are all ids the second knows -- which is what stops a chip
    // being refused because the two drifted apart.
    const drift = await page.evaluate(async () => {
      const { everyBet } = await import("/public/roulette-board.js");
      const known = new Set(
        JSON.parse(document.getElementById("roulette-board").textContent).map((spec) => spec.id),
      );
      return { known: known.size, unknown: everyBet().filter((id) => !known.has(id)) };
    });
    expect(drift.known, "the page ships the whole catalogue").toBe(157);
    expect(drift.unknown, "every bet the board can aim at must be one the server prices").toEqual([]);
  });

  test("aiming names the bet under the finger before it costs anything", async ({ page }) => {
    await sitDown(page);
    // The middle of a square is that number; its edges and corners are the
    // lines between squares, exactly as a chip would rest on real felt.
    expect(await drop(page, 17, 0.5, 0.5)).toContain("Straight up");
    expect(await drop(page, 17, 0.5, 0.98)).toContain("Split");
    expect(await drop(page, 14, 0.98, 0.5)).toContain("Split");
    expect(await drop(page, 22, 0.02, 0.5)).toContain("Street");
    expect(await drop(page, 25, 0.98, 0.98)).toContain("Corner");
    expect(await drop(page, 31, 0.02, 0.98)).toContain("Six line");
    expect(await drop(page, 1, 0.98, 0.02)).toContain("Trio");
    expect(await drop(page, 1, 0.02, 0.02)).toContain("First four");
    // Eight chips at $5, and the stack has not moved: chips on the felt are a
    // claim on it, not a withdrawal from it.
    await expect(page.locator(".rl-money")).toContainText("$40.00");
    await expect(page.locator(".rl-money")).toContainText("$1,000.00");
    await expect(page.locator(".rl-chip")).toHaveCount(8);
  });

  test("chips sit on the exact spot or line that was aimed at", async ({ page }) => {
    await sitDown(page);
    const centre = await square(page, 17);
    await drop(page, 17, 0.5, 0.5);
    let chip = await page.locator(".rl-chip").boundingBox();
    expect(chip.x + chip.width / 2).toBeCloseTo(centre.x + centre.width / 2, 0);
    expect(chip.y + chip.height / 2).toBeCloseTo(centre.y + centre.height / 2, 0);

    await page.getByRole("button", { name: "Clear" }).click();
    const cell = await square(page, 17);
    const across = (await page.locator(".rl-board.across").count()) > 0;
    await drop(page, 17, 0.5, 0.98);
    chip = await page.locator(".rl-chip").boundingBox();
    const expected = across
      ? { x: cell.x + cell.width, y: cell.y + cell.height / 2 }
      : { x: cell.x + cell.width / 2, y: cell.y + cell.height };
    expect(Math.abs(chip.x + chip.width / 2 - expected.x)).toBeLessThanOrEqual(1);
    expect(Math.abs(chip.y + chip.height / 2 - expected.y)).toBeLessThanOrEqual(1);
  });

  test("the compact wheel zooms on hover", async ({ page }) => {
    await sitDown(page);
    const wheel = page.locator(".rl-wheel-dock");
    const resting = await wheel.boundingBox();
    await page.getByRole("button", { name: "Spin the roulette wheel" }).hover();
    await page.waitForTimeout(250);
    const zoomed = await wheel.boundingBox();
    expect(zoomed.width).toBeGreaterThan(resting.width * 1.5);
  });

  test("a spin pays what the board says it pays", async ({ page }) => {
    await sitDown(page);
    await page.locator('[data-cell="red"]').click();
    await page.locator('[data-cell="black"]').click();
    // Backing both colours cannot win and cannot lose: one of them is paid
    // even money and the other is taken, unless the zero comes up.
    await expect(page.locator(".rl-money")).toContainText("$10.00");
    await page.getByRole("button", { name: "Spin the roulette wheel" }).click();
    await expect(page.locator(".rl-result")).toBeVisible({ timeout: 25_000 });
    await expect(page.locator(".rl-wheel-dock")).toHaveClass(/open/);
    await expect(page.locator(".rl-felt")).toBeVisible();
    const settled = await page.evaluate(async () => {
      const state = await (await fetch("/roulette/state", { headers: { Accept: "application/json" } })).json();
      return { stack: state.stack, last: state.last, staked: state.staked };
    });
    expect(settled.staked, "the felt is swept for the next spin").toBe(0);
    const green = settled.last.number === 0;
    expect(settled.last.returned).toBe(green ? 0 : 1_000);
    expect(settled.stack).toBe(green ? 99_000 : 100_000);
    expect(settled.last.staked).toBe(1_000);
  });

  test("the felt refuses a chip the stack cannot cover", async ({ page }) => {
    await sitDown(page);
    await page.locator(".rl-chip-button", { hasText: "$100.00" }).click();
    // Two hundred dollars is the ceiling on any one spot.
    await page.locator('[data-cell="red"]').click();
    await page.locator('[data-cell="red"]').click();
    await expect(page.locator(".rl-money")).toContainText("$200.00");
    await page.locator('[data-cell="red"]').click();
    await expect(page.locator(".error")).toContainText("limit");
    await expect(page.locator(".rl-money")).toContainText("$200.00");
  });

  test("undo lifts the last chip and rebet puts the layout back", async ({ page }) => {
    await sitDown(page);
    await page.locator('[data-cell="red"]').click();
    await drop(page, 17, 0.5, 0.5);
    await expect(page.locator(".rl-chip")).toHaveCount(2);
    await page.getByRole("button", { name: "Undo" }).click();
    await expect(page.locator(".rl-chip")).toHaveCount(1);
    await page.getByRole("button", { name: "Clear" }).click();
    await expect(page.locator(".rl-chip")).toHaveCount(0);

    await page.locator('[data-cell="red"]').click();
    await page.getByRole("button", { name: "Spin the roulette wheel" }).click();
    await expect(page.locator(".rl-result")).toBeVisible({ timeout: 25_000 });
    await page.getByRole("button", { name: "Rebet" }).click();
    await expect(page.locator(".rl-money")).toContainText("$5.00");
  });

  test("the felt is laid out the way the screen has room for", async ({ page }) => {
    await sitDown(page);
    const wide = page.viewportSize().width >= 56 * 16;
    const across = (await page.locator(".rl-board.across").count()) > 0;
    expect(across, "a croupier's layout needs the width for twelve columns").toBe(wide);
    // Either way it is the same board: the zero, thirty-six numbers, three
    // dozens, three column bets and the six even-money spots.
    await expect(page.locator(".rl-board [data-cell]")).toHaveCount(37 + 3 + 3 + 6);
    const one = await square(page, 1);
    const two = await square(page, 2);
    const four = await square(page, 4);
    if (across) {
      // 1, 2, 3 climb the left-hand column; 1 and 4 sit side by side.
      expect(two.y).toBeLessThan(one.y);
      expect(four.x).toBeGreaterThan(one.x);
      expect(Math.round(four.y)).toBe(Math.round(one.y));
    } else {
      expect(two.x).toBeGreaterThan(one.x);
      expect(four.y).toBeGreaterThan(one.y);
      expect(Math.round(four.x)).toBe(Math.round(one.x));
    }
  });

  test("the table holds still while you aim at it", async ({ page }) => {
    await sitDown(page);
    // The felt is sized from whatever the row above it leaves, so a status line
    // that grows by a line shrinks every square on the board -- which reads as
    // the table twitching under the thumb that is trying to aim at it.
    const height = async () => (await page.locator(".rl-board").boundingBox()).height;
    const settled = await height();
    const seen = [settled];
    // Aiming is a sequence of pointer moves; they cannot be raced.
    /* oxlint-disable no-await-in-loop */
    for (const [n, fx, fy] of [[17, 0.5, 0.5], [31, 0.02, 0.98], [25, 0.98, 0.98]] as const) {
      const box = await square(page, n);
      const across = (await page.locator(".rl-board.across").count()) > 0;
      const [sx, sy] = across ? [fy, 1 - fx] : [fx, fy];
      await page.mouse.move(box.x + box.width * sx, box.y + box.height * sy);
      seen.push(await height());
    }
    /* oxlint-enable no-await-in-loop */
    await page.mouse.move(2, 2);
    seen.push(await height());
    expect(Math.max(...seen) - Math.min(...seen), "the board must not resize as you aim").toBe(0);

    // Nor when a result lands and the marquee gains a number.
    await page.locator('[data-cell="red"]').click();
    await page.getByRole("button", { name: "Spin the roulette wheel" }).click();
    await expect(page.locator(".rl-result")).toBeVisible({ timeout: 25_000 });
    await page.waitForTimeout(2800);
    expect(await height(), "nor when a spin settles").toBe(settled);
  });

  test("the table fits the phone it is played on", async ({ page }) => {
    await sitDown(page);
    // The board takes every touch so a thumb can slide onto a line, which means
    // it cannot be scrolled past -- so nothing may need scrolling to reach.
    const room = await page.evaluate(() => ({
      over: document.documentElement.scrollHeight - window.innerHeight,
      wide: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    }));
    expect(room.over, "the table must not need scrolling").toBeLessThanOrEqual(0);
    expect(room.wide, "and must not run off the side").toBeLessThanOrEqual(0);

    // Measured rather than pinned to a baseline: what matters here is that the
    // whole felt and every control are reachable at whatever size the viewport
    // gave them, and that holds on any host. An absolute-coordinate snapshot of
    // this page would also move with a line of copy above it.
    await expect(page.locator(".rl-board [data-cell]")).toHaveCount(37 + 3 + 3 + 6);
    const box = await page.locator(".rl-board").boundingBox();
    const toolbar = await page.locator(".rl-toolbar").boundingBox();
    const view = page.viewportSize();
    expect(toolbar.y + toolbar.height, "the controls must sit on screen").toBeLessThanOrEqual(view.height);
    // A square has to be worth aiming at with a thumb.
    // The felt gets whatever the controls leave, and what matters about the
    // result is that a square is still worth aiming at with a thumb -- which is
    // the same requirement whichever way the board is hung, and the reason the
    // rows are fractions of the space rather than a fixed size.
    const square17 = await square(page, 17);
    expect(square17.height, "a square must stay tall enough to aim at").toBeGreaterThanOrEqual(25);
    expect(square17.width, "and wide enough").toBeGreaterThanOrEqual(60);
    // Against the room it is given rather than the viewport: the table is
    // capped, because a board two thousand pixels across is not a nicer board.
    const stage = await page.locator(".rl-felt").boundingBox();
    expect(box.width, "the felt should take the width it is given").toBeGreaterThan(stage.width * 0.9);
  });
});
