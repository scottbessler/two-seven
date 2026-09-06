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

  test("holds its layout", async ({ page }) => {
    await page.goto("/roulette-test");
    await page.waitForFunction(() => Boolean(window.rouletteWheel));
    await expectLayout(page, "roulette-page", LAYOUT);
  });
});
