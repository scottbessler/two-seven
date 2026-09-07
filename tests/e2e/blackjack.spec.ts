import { expect, test } from "./fixtures";
import type { Page } from "@playwright/test";

let account = 0;
async function signIn(page, name: string) {
  account += 1;
  await page.goto("/");
  const suffix = `${Date.now()}${account}${Math.random().toString(36).slice(2, 7)}`;
  await page.fill('#register-form input[name="username"]', `${name}${suffix}`);
  await page.fill('#register-form input[name="display_name"]', name);
  await page.click("#register-form button");
  await page.waitForTimeout(300);
}
// A look at the table for whoever is reading the run, filed with the rest of
// the run's artifacts rather than at a path from the machine it was written on.
// The path comes from `test.info()` rather than a threaded parameter: a helper
// every test calls should not make each of them remember to pass something.
async function shot(page, name: string) {
  await page.screenshot({ path: test.info().outputPath(`blackjack-${name}.png`), fullPage: true });
}

// Sits down at the lowest ceiling the slider offers, which is $100 whatever
// the bank holds.
async function sitDown(page: Page, maxBet: string) {
  await page.goto("/blackjack");
  await expect(page.locator(".blackjack-sit-slider")).toBeVisible();
  await page.locator(".blackjack-sit-slider").fill("0");
  await expect(page.locator(".blackjack-sit-stakes")).toContainText(maxBet);
  await page.getByRole("button", { name: /Sit down/ }).click();
  await expect(page.getByText("your bank")).toBeVisible();
}

// Plays the hand to completion — declining insurance and standing — until the
// game has settled and reopened for betting.
async function finishRound(page: Page): Promise<void> {
  await expect.poll(async () => {
    const state = await (await page.request.get("/blackjack/state")).json();
    if (state.can_bet) return true;
    const decline = page.getByRole("button", { name: "No insurance" });
    const stand = page.getByRole("button", { name: "Stand" });
    if (await decline.count()) await decline.first().click();
    else if (await stand.count()) await stand.first().click();
    return false;
  }, { timeout: 20_000, intervals: [250] }).toBe(true);
}

test("the slider runs from $100 to the whole bank and nothing is bought in", async ({ page }) => {
  await signIn(page, "Slider");
  await page.goto("/blackjack");
  // A new account has nothing, and the cheapest ceiling is $100.
  await expect(page.locator(".blackjack-sit-slider")).toHaveCount(0);
  await expect(page.getByText("You need $100 in the bank to sit down.")).toBeVisible();
  const before = (await (await page.request.get("/api/bank")).json()).balance;
  await page.request.post("/api/bank", { data: {} });
  await page.goto("/blackjack");
  // One re-up is $1,000, so the ladder runs $100 to the whole $1,000.
  await expect(page.locator(".blackjack-sit-scale")).toHaveText("$100$1,000");
  await expect(page.locator(".blackjack-sit-stakes")).toContainText("$100");
  await expect(page.locator(".blackjack-sit-stakes")).toContainText("$20");
  await expect(page.getByRole("button", { name: "Sit down", exact: true })).toBeVisible();
  await expect(page.locator(".blackjack-sit-note")).toHaveText("Five wagers, $20 to $100.");
  await shot(page, "sit-down");
  // Dragging to the top asks for the whole bank as the ceiling.
  await page.locator(".blackjack-sit-slider").fill("3");
  await expect(page.locator(".blackjack-sit-stakes")).toContainText("$1,000");
  await page.getByRole("button", { name: "Sit down", exact: true }).click();
  await expect(page.getByText("your bank")).toBeVisible();
  // Taking the seat cost nothing: the bank is exactly the re-up.
  expect((await (await page.request.get("/api/bank")).json()).balance).toBe(before + 100_000);
});

test("a player sits down, sees fixed wagers and is dealt at once", async ({ page }) => {
  await signIn(page, "Solo");
  await page.request.post("/api/bank", { data: {} });
  await sitDown(page, "$100");
  await expect(page.locator(".turn-clock")).toHaveCount(0);
  await shot(page, "betting");
  /* oxlint-disable no-await-in-loop */
  for (const label of ["Bet $20", "Bet $40", "Bet $60", "Bet $80", "Bet $100"]) await expect(page.getByRole("button", { name: label })).toBeVisible();
  /* oxlint-enable no-await-in-loop */
  await Promise.all([
    page.waitForResponse((response) => response.url().includes("/blackjack/bet") && response.request().method() === "POST"),
    page.getByRole("button", { name: "Bet $20" }).click(),
  ]);
  // A solo bet deals in the same request that placed it.
  const state = await (await page.request.get("/blackjack/state")).json();
  expect(state.phase).not.toBe("betting");
  if (await page.getByRole("button", { name: "No insurance" }).count()) await page.getByRole("button", { name: "No insurance" }).click();
  // The deal decides whether there is a turn to take: a natural on either side
  // settles the hand before the player is asked for anything.
  if (await page.getByRole("button", { name: "Stand" }).count()) {
    await expect(page.locator(".blackjack-player-hand.active")).toBeVisible();
    // The turn announcement is for screen readers only.
    await expect(page.locator(".blackjack-turn-announcement")).toHaveText("Your move");
    await expect(page.locator(".blackjack-turn-announcement")).toHaveCSS("opacity", "0");
  }
  await expect(page.locator(".blackjack-player-summary")).toContainText("You");
  const playerLayout = await page.locator(".blackjack-player-hand").evaluate((hand) => {
    const tray = hand.getBoundingClientRect();
    const cards = hand.querySelector<HTMLElement>(".board")!.getBoundingClientRect();
    const summary = hand.querySelector<HTMLElement>(".blackjack-player-summary")!.getBoundingClientRect();
    return {
      centerDelta: Math.abs((cards.left + summary.right - tray.left - tray.right) / 2),
      gap: summary.left - cards.right,
    };
  });
  expect(playerLayout.centerDelta, "cards and player info should be centered together").toBeLessThanOrEqual(2);
  expect(playerLayout.gap, "player info should sit directly beside the cards").toBeLessThanOrEqual(16);
  await expect(page.locator(".blackjack-dealer-hand")).toHaveAttribute("aria-label", /Dealer/);
  await shot(page, "player-tray");
  await finishRound(page);
  // A settled round stays on the felt: nothing clears it but the next bet.
  await expect(page.locator(".blackjack-player-hand")).toBeVisible();
  await page.getByRole("button", { name: "Leave table" }).click();
  await expect(page.locator(".blackjack-sit")).toBeVisible();
});

test("a round moves the bank and leaving moves nothing", async ({ page }) => {
  await signIn(page, "Leaving");
  await page.request.post("/api/bank", { data: {} });
  const before = (await (await page.request.get("/api/bank")).json()).balance;
  await sitDown(page, "$100");
  // Sitting down is free.
  expect((await (await page.request.get("/api/bank")).json()).balance).toBe(before);
  // A wager leaves the bank the moment it is staked.
  await Promise.all([
    page.waitForResponse((response) => response.url().includes("/blackjack/bet") && response.request().method() === "POST"),
    page.getByRole("button", { name: "Bet $100" }).click(),
  ]);
  await finishRound(page);
  const settled = (await (await page.request.get("/api/bank")).json()).balance;
  // Won, lost or pushed, the round is worth a whole wager either way.
  expect([before - 10_000, before, before + 10_000, before + 15_000]).toContain(settled);
  await page.getByRole("button", { name: "Leave table" }).click();
  await expect(page.locator(".blackjack-sit-slider")).toBeVisible();
  // Getting up is free too.
  expect((await (await page.request.get("/api/bank")).json()).balance).toBe(settled);
});

// The phone's own layout is measured in `safe-area.spec.ts`, against the
// iPhone this ships to and its real insets. It used to be checked here on a
// 412x915 viewport with every inset at zero -- the notchless phone `B9` was
// recorded for -- and only for sideways scroll, which a clipped shell never
// causes.
