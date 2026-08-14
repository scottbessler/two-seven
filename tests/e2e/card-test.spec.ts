import { expect, test } from "@playwright/test";

test.describe("card test page", () => {
  test("renders the full deck without obvious card-face regressions", async ({ page }) => {
    await page.goto("/card-test");
    await expect(page.locator(".playing-card")).toHaveCount(52);
    // The face is rank over suit only: no pips, no court art, no second corner.
    await expect(page.locator(".card-corner")).toHaveCount(52);
    await expect(page.locator(".pip-grid, .card-art, .court-piece, .card-frame, .card-corner-bottom")).toHaveCount(0);
    const cardFaceStyles = await page.evaluate(() => {
      const corner = document.querySelector(".card-corner b");
      const suit = document.querySelector(".card-corner i");
      const card = document.querySelector(".playing-card");
      const width = Number.parseFloat(getComputedStyle(card).width);
      return {
        cornerWeight: Number(getComputedStyle(corner).fontWeight),
        cornerToCardRatio: Number.parseFloat(getComputedStyle(corner).fontSize) / width,
        rankSize: Number.parseFloat(getComputedStyle(corner).fontSize),
        suitSize: Number.parseFloat(getComputedStyle(suit).fontSize),
        faceHeight: corner.getBoundingClientRect().height + suit.getBoundingClientRect().height,
        cardHeight: Number.parseFloat(getComputedStyle(card).height),
      };
    });
    expect(cardFaceStyles.cornerWeight).toBeGreaterThanOrEqual(700);
    expect(cardFaceStyles.cornerToCardRatio).toBeGreaterThanOrEqual(0.19);
    expect(cardFaceStyles.rankSize, "rank and suit must render at one size").toBe(cardFaceStyles.suitSize);
    expect(cardFaceStyles.faceHeight, "the rank and suit stack must fit the card").toBeLessThan(cardFaceStyles.cardHeight);
    const overflowingSuitRows = await page.locator(".card-test-grid").evaluateAll((rows) =>
      rows.filter((row) => row.scrollWidth > row.clientWidth).length,
    );
    expect(overflowingSuitRows, "V8: card test suit rows must not scroll horizontally").toBe(0);
    await expect(page).toHaveScreenshot("card-test.png", { fullPage: true });
  });
});
