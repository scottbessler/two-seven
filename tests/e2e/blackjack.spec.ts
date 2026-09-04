import { expect, test } from "./fixtures";

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
async function shot(page, name: string) {
  const suffix = (await page.evaluate(() => innerWidth)) < 700 ? "mobile" : "desktop";
  await page.screenshot({ path: `/home/ubuntu/shots/blackjack-${suffix}-${name}.png`, fullPage: true });
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

test("a solo player sees fixed wagers and deals immediately", async ({ page }) => {
  await signIn(page, "Solo");
  await page.request.post("/api/bank", { data: {} });
  const url = await tableUrl(page, 0);
  await page.goto(url!);
  await page.getByRole("button", { name: /Sit down · \$1,000/ }).click();
  await expect(page.getByText("Your chips")).toBeVisible();
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
  await expect(page.locator(".turn-clock")).toHaveCount(0);
  if (await page.getByRole("button", { name: "No insurance" }).count()) await page.getByRole("button", { name: "No insurance" }).click();
  await expect.poll(async () => (await page.getByRole("button", { name: /Hit|Stand/ }).count()) > 0 || (await (await page.request.get(stateUrl)).json()).phase === "settled", { timeout: 12_000 }).toBe(true);
  await page.goto(url);
  if (await page.getByRole("button", { name: "Stand" }).count()) await page.getByRole("button", { name: "Stand" }).click();
  await expect.poll(async () => (await (await page.request.get(stateUrl)).json()).phase === "betting", { timeout: 12_000 }).toBe(true);
  await page.goto(url);
  await page.getByRole("button", { name: "Leave table" }).click();
});

test("two players share a table and the unbet player sits out", async ({ browser }) => {
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
  await expect(second.locator(".blackjack-seat").filter({ hasText: "Alice" })).toBeVisible();
  await first.getByRole("button", { name: "Bet $25" }).click();
  await expect(second.locator(".turn-clock")).toBeVisible();
  await shot(second, "mid-round-two-player");
  const stateUrl = `${url}/state`;
  await expect.poll(async () => {
    const response = await second.request.get(stateUrl);
    const state = await response.json();
    return state.seats.some((seat: { user: string; waiting: boolean }) => seat.waiting);
  }, { timeout: 16_000 }).toBe(true);
  await expect.poll(async () => (await second.locator("body").innerText()).includes("Sitting out"), { timeout: 16_000 }).toBe(true);
  await first.goto(url);
  await first.getByRole("button", { name: "Stand" }).click();
  await expect.poll(async () => (await (await first.request.get(stateUrl)).json()).phase === "betting", { timeout: 12_000 }).toBe(true);
  await first.goto(url);
  await first.getByRole("button", { name: "Leave table" }).click();
  await second.goto(url);
  await second.getByRole("button", { name: "Leave table" }).click();
  await first.close(); await second.close();
});

test("leaving a blackjack table returns to the lobby", async ({ page }) => {
  await signIn(page, "Leaving");
  await page.request.post("/api/bank", { data: {} });
  const url = await tableUrl(page, 0);
  await page.goto(url!);
  await page.getByRole("button", { name: /Sit down/ }).click();
  await page.getByRole("button", { name: "Leave table" }).click();
  await expect(page).toHaveURL(/\/blackjack$/);
  await expect(page.locator(".lobby")).toBeVisible();
});

test("blackjack mobile keeps the shared table inside the viewport", async ({ page }) => {
  await page.setViewportSize({ width: 412, height: 915 });
  await signIn(page, "Mobile");
  await page.request.post("/api/bank", { data: {} });
  const url = await tableUrl(page, 0);
  await page.goto(url!);
  await page.getByRole("button", { name: /Sit down/ }).click();
  await expect(page.locator(".blackjack-table")).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  expect(overflow).toBe(false);
  await page.getByRole("button", { name: "Leave table" }).click();
});
