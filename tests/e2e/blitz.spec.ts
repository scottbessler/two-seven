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

test("hand blitz buy-ins refresh the header balance", async ({ page }) => {
  await signIn(page, "blitzbank");
  await page.request.post("/api/bank", { data: {} });
  await page.goto("/hand-blitz");
  await expect(page.locator("#bank-balance")).toHaveText("$1,000");

  await page.getByRole("button", { name: /Easy/ }).click();

  await expect(page.locator("#bank-balance")).toHaveText("$990");
  await expect(page.locator(".blitz-table")).toBeVisible();
});
