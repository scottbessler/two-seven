import { expect, test } from "./fixtures";

let accounts = 0;

async function signIn(page, name) {
  // Both projects share one server, so a username has to be unique per run.
  accounts += 1;
  await page.goto("/");
  await page.fill('#register-form input[name="username"]', `${name}${Date.now()}${accounts}${Math.random().toString(36).slice(2, 8)}`);
  await page.fill('#register-form input[name="display_name"]', name);
  await page.click("#register-form button");
  await page.waitForTimeout(600);
}

test("hands another player money in $1,000 chips", async ({ page }) => {
  await signIn(page, "Taker");
  await page.goto("/player");
  const taker = await page.locator(".player-page").getAttribute("data-player-id");
  await expect(page.locator(".gift-panel")).toHaveCount(0);
  await page.request.post("/auth/logout");

  await signIn(page, "Giver");
  // One re-up: exactly one chip to give, and no way to offer a second.
  await page.request.post("/api/bank", { data: {} });
  await page.goto(`/player/${taker}`);
  const send = page.locator(".gift-send");
  await expect(send).toHaveText("Send $1,000");
  await expect(page.locator('.gift-step[data-step="1"]')).toBeDisabled();
  await expect(page.locator('.gift-step[data-step="-1"]')).toBeDisabled();

  await send.click();

  await expect(page.locator(".gift-status")).toHaveText("Sent $1,000 to Taker.");
  await expect(page.locator(".gift-balance")).toHaveText("$0.00");
  // The header total and the recipient's ledger both catch up on the spot.
  await expect(page.locator("#bank-balance")).toHaveText("$0");
  await expect(page.locator(".ledger-panel")).toContainText("gift from Giver");
  await expect(send).toBeDisabled();

  // The chip shows up netted per person, from both sides: the page you are
  // looking at counts it as money in, your own counts it as money out.
  await expect(page.locator(".gifts-panel tbody tr")).toContainText(["Giver"]);
  await expect(page.locator(".gifts-panel .money.positive")).toHaveText("+$1,000.00");
  await page.goto("/player");
  await expect(page.locator(".gifts-panel tbody tr")).toContainText(["Taker"]);
  await expect(page.locator(".gifts-panel .money.negative")).toHaveText("-$1,000.00");
});

test("account options save to the account and come back on reload", async ({ page }) => {
  await signIn(page, "Optioneer");
  await page.goto("/player");
  const options = page.locator(".options-panel");
  const botCards = options.locator("[name=see-bot-cards]");
  await expect(botCards).not.toBeChecked();

  await botCards.check();
  await expect(options.locator(".option-status")).toHaveText("Saved.");

  // The server is what enforces these, so the reload is the real test.
  await page.reload();
  await expect(page.locator("[name=see-bot-cards]")).toBeChecked();
  await expect(page.locator("[name=unfunded-tournaments]")).not.toBeChecked();
});
