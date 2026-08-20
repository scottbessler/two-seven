import { expect, test } from "@playwright/test";

let accounts = 0;

async function signIn(page, name) {
  accounts += 1;
  await page.goto("/");
  await page.fill('#register-form input[name="username"]', `${name}${Date.now()}${accounts}${Math.random().toString(36).slice(2, 8)}`);
  await page.fill('#register-form input[name="display_name"]', name);
  await page.click("#register-form button");
  await page.waitForTimeout(600);
}

test("blackjack notices a same-page re-up", async ({ page }) => {
  await signIn(page, "blackjackbroke");
  await page.goto("/blackjack");
  await expect(page.locator(".deal-broke")).toContainText("Re-up from the coin menu");

  await page.locator(".bank-widget summary").click();
  await page.locator(".re-up-button").click();

  await expect(page.getByRole("button", { name: "Deal $10" })).toBeVisible();
  await expect(page.locator("#bank-balance")).toHaveText("$1,000");
});

test("blackjack bank mutations refresh the header balance", async ({ page }) => {
  await signIn(page, "blackjackbank");
  await page.request.post("/api/bank", { data: {} });
  await page.goto("/blackjack");
  await expect(page.locator("#bank-balance")).toHaveText("$1,000");

  await page.getByRole("button", { name: "Deal $10" }).click();

  await expect(page.locator("#bank-balance")).toHaveText("$990");
});
