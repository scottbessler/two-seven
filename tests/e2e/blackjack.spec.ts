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
async function tableUrl(page, index = 0) {
  await page.goto("/blackjack");
  return page.locator('a[href^="/blackjack/tables/"]').nth(index).getAttribute("href");
}
// A look at the table for whoever is reading the run, filed with the rest of
// the run's artifacts rather than at a path from the machine it was written on.
// The path comes from `test.info()` rather than a threaded parameter: a helper
// every test calls should not make each of them remember to pass something.
async function shot(page, name: string) {
  await page.screenshot({ path: test.info().outputPath(`blackjack-${name}.png`), fullPage: true });
}

test("blackjack lobby lists all four fixed tiers", async ({ page }) => {
  await signIn(page, "Lobby");
  await page.goto("/blackjack");
  /* oxlint-disable no-await-in-loop */
  for (const label of ["Max bet $100", "Max bet $1,000", "Max bet $10,000", "Max bet $100,000", "Buy-in $1,000", "Buy-in $10,000", "Buy-in $100,000", "Buy-in $1,000,000"]) {
    await expect(page.locator("h2, p").filter({ hasText: label }).first()).toBeVisible();
  }
  /* oxlint-enable no-await-in-loop */
  await expect(page.locator('a[href^="/blackjack/tables/"]')).toHaveCount(4);
  await shot(page, "lobby");
});

// The four blackjack tables are fixed and shared by every signed-in user, so
// the table tests run once, in one project, to keep them from seating each
// other's players mid-round. The lobby test is read-only and runs everywhere.
// Plays the viewer's hands to completion — declining insurance and standing —
// until the table has settled and reopened for betting.
async function finishRound(page: Page, url: string): Promise<void> {
  const stateUrl = `${url}/state`;
  await expect.poll(async () => {
    const state = await (await page.request.get(stateUrl)).json();
    if (state.phase === "betting") return true;
    const decline = page.getByRole("button", { name: "No insurance" });
    const stand = page.getByRole("button", { name: "Stand" });
    if (await decline.count()) await decline.first().click();
    else if (await stand.count()) await stand.first().click();
    return false;
  }, { timeout: 20_000, intervals: [250] }).toBe(true);
}

const tableTests = test.extend({});
tableTests.skip(({ isMobile }) => Boolean(isMobile), "shared tables are exercised once, on desktop");

tableTests("a solo player sees fixed wagers and deals immediately", async ({ page }) => {
  await signIn(page, "Solo");
  await page.request.post("/api/bank", { data: {} });
  const url = await tableUrl(page, 0);
  await page.goto(url!);
  await page.getByRole("button", { name: /Sit down · \$1,000/ }).click();
  await expect(page.getByText("your chips")).toBeVisible();
  await expect(page.locator(".turn-clock")).toHaveCount(0);
  await shot(page, "betting");
  /* oxlint-disable no-await-in-loop */
  for (const label of ["Bet $25", "Bet $50", "Bet $75", "Bet $100"]) await expect(page.getByRole("button", { name: label })).toBeVisible();
  /* oxlint-enable no-await-in-loop */
  await Promise.all([
    page.waitForResponse((response) => response.url().includes("/bet") && response.request().method() === "POST"),
    page.getByRole("button", { name: "Bet $25" }).click(),
  ]);
  const stateUrl = `${url}/state`;
  await expect.poll(async () => (await (await page.request.get(stateUrl)).json()).phase !== "betting", { timeout: 5_000 }).toBe(true);
  if (await page.getByRole("button", { name: "No insurance" }).count()) await page.getByRole("button", { name: "No insurance" }).click();
  if (await page.getByRole("button", { name: "Stand" }).count()) await expect(page.locator(".blackjack-player-hand.active")).toBeVisible();
  await expect(page.locator(".blackjack-player-summary")).toContainText("You");
  await expect(page.getByText("Your turn", { exact: true })).toHaveCSS("opacity", "0");
  await expect(page.locator(".blackjack-dealer-hand")).toHaveAttribute("aria-label", /Dealer/);
  await shot(page, "player-tray");
  await finishRound(page, url);
  await page.getByRole("button", { name: "Leave table" }).click();
});

tableTests("two players share a table and the unbet player sits out", async ({ browser }) => {
  const first = await browser.newPage();
  const second = await browser.newPage();
  await signIn(first, "Alice");
  await signIn(second, "Bob");
  await first.request.post("/api/bank", { data: {} });
  await second.request.post("/api/bank", { data: {} });
  const url = await tableUrl(first, 0);
  await first.goto(url!); await second.goto(url!);
  await first.getByRole("button", { name: /Sit down/ }).click();
  await second.getByRole("button", { name: /Sit down/ }).click();
  await expect(second.locator(".blackjack-seat").filter({ hasText: "Alice" }).first()).toBeVisible();
  await first.getByRole("button", { name: "Bet $25" }).click();
  await expect(second.locator(".turn-clock")).toBeVisible();
  await shot(second, "mid-round-two-player");
  const stateUrl = `${url}/state`;
  await expect.poll(async () => {
    const response = await second.request.get(stateUrl);
    const state = await response.json();
    return state.phase !== "betting" && state.seats.some((seat: { waiting: boolean }) => seat.waiting);
  }, { timeout: 16_000 }).toBe(true);
  await expect(second.locator(".blackjack-own-note")).toHaveText("Sitting this round out");
  await finishRound(first, url);
  await first.getByRole("button", { name: "Leave table" }).click();
  await second.goto(url);
  await second.getByRole("button", { name: "Leave table" }).click();
  await first.close(); await second.close();
});

tableTests("leaving a blackjack table returns to the lobby", async ({ page }) => {
  await signIn(page, "Leaving");
  await page.request.post("/api/bank", { data: {} });
  const url = await tableUrl(page, 0);
  await page.goto(url!);
  await page.getByRole("button", { name: /Sit down/ }).click();
  await page.getByRole("button", { name: "Leave table" }).click();
  await expect(page).toHaveURL(/\/blackjack$/);
  await expect(page.locator(".lobby")).toBeVisible();
});

// The phone's own layout is measured in `safe-area.spec.ts`, against the
// iPhone this ships to and its real insets. It used to be checked here on a
// 412x915 viewport with every inset at zero -- the notchless phone `B9` was
// recorded for -- and only for sideways scroll, which a clipped shell never
// causes.
