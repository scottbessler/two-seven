import { expect, test } from "@playwright/test";

test.describe("card test page", () => {
  test("renders the full deck without obvious card-face regressions", async ({ page }) => {
    await page.goto("/card-test");
    await expect(page.locator(".playing-card")).toHaveCount(52);
    await expect(page.locator(".pip-grid-10")).toHaveCount(4);
    await expect(page.locator(".card-art-A")).toHaveCount(4);
    await expect(page.locator(".card-art-K")).toHaveCount(4);
    await expect(page.locator(".court-piece")).toHaveCount(12);
    const cardFaceStyles = await page.evaluate(() => {
      const corner = document.querySelector(".card-corner b");
      const card = document.querySelector(".playing-card");
      const ace = document.querySelector(".card-art-A");
      const court = document.querySelector(".card-art-K");
      return {
        cornerWeight: corner ? Number(getComputedStyle(corner).fontWeight) : 0,
        cornerToCardRatio:
          corner && card
            ? Number.parseFloat(getComputedStyle(corner).fontSize) /
              Number.parseFloat(getComputedStyle(card).width)
            : 0,
        aceBefore: ace ? getComputedStyle(ace, "::before").display : "missing",
        aceAfter: ace ? getComputedStyle(ace, "::after").display : "missing",
        courtBefore: court ? getComputedStyle(court, "::before").display : "missing",
        courtAfter: court ? getComputedStyle(court, "::after").display : "missing",
      };
    });
    expect(cardFaceStyles.cornerWeight).toBeGreaterThanOrEqual(700);
    expect(cardFaceStyles.cornerToCardRatio).toBeGreaterThanOrEqual(0.19);
    expect(cardFaceStyles.aceBefore).toBe("none");
    expect(cardFaceStyles.aceAfter).toBe("none");
    expect(cardFaceStyles.courtBefore).toBe("none");
    expect(cardFaceStyles.courtAfter).toBe("none");
    const overflowingSuitRows = await page.locator(".card-test-grid").evaluateAll((rows) =>
      rows.filter((row) => row.scrollWidth > row.clientWidth).length,
    );
    expect(overflowingSuitRows, "V8: card test suit rows must not scroll horizontally").toBe(0);
    await expect(page).toHaveScreenshot("card-test.png", { fullPage: true });
  });
});
