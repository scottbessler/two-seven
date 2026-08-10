import { expect, test } from "@playwright/test";

test.describe("card test page", () => {
  test("renders the full deck without obvious card-face regressions", async ({ page }) => {
    await page.goto("/card-test");
    await expect(page.locator(".playing-card")).toHaveCount(52);
    await expect(page.locator(".pip-grid-10")).toHaveCount(4);
    await expect(page.locator(".card-art-A")).toHaveCount(4);
    await expect(page.locator(".card-art-K")).toHaveCount(4);
    await expect(page).toHaveScreenshot("card-test.png", { fullPage: true });
  });
});
