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

test("blackjack trainer settings drive tutor quiz and analyzer", async ({ page }) => {
  await signIn(page, "blackjacktrainer");
  await page.request.post("/api/bank", { data: {} });
  let startPayload;
  await page.route("**/blackjack/start", async (route) => {
    startPayload = route.request().postDataJSON();
    await route.fulfill({
      json: {
        id: "00000000-0000-0000-0000-000000000001",
        bet: 1000,
        player: ["Ts", "6s"],
        dealer: ["Th", "7d"],
        player_score: 16,
        dealer_score: 17,
        status: "DealerWin",
        message: "Dealer wins.",
        payout: 0,
        can_hit: false,
        can_stand: false,
        can_double: false,
        can_split: false,
        can_insure: false,
        insurance: 0,
        hands: [{ cards: ["Ts", "6s"], bet: 1000, score: 16, status: "Loss", blackjack: false }],
        active_hand: 1,
        settings: {
          decks: 1,
          penetration_percent: 80,
          counting_tutor: true,
          counting_quiz: true,
          bet_analyzer: true,
        },
        count: { running: -3, true_count: -3.3, visible_cards: 4, penetration_percent: 7 },
        trainer_log: ["Dealer up Th: -1 -> -1", "Hand 1 6s: +1 -> 0", "Dealer 7d: 0 -> -3"],
        quiz: { prompt: "What is the running count?", choices: [-4, -3, -2, -1], answer: -3 },
        analysis: ["Stand was off; basic strategy prefers Hit here."],
      },
    });
  });
  await page.goto("/blackjack");
  await page.getByRole("button", { name: "Card display settings" }).click();
  await page.locator('select[name="blackjack-decks"]').selectOption("1");
  await page.locator('input[name="blackjack-penetration"]').fill("80");
  await page.locator('input[name="counting-tutor"]').check();
  await page.locator('input[name="counting-quiz"]').check();
  await page.locator('input[name="bet-analyzer"]').check();
  await page.getByRole("button", { name: "Close" }).click();

  await page.getByRole("button", { name: "Deal $10" }).click();

  expect(startPayload.settings).toEqual({
    decks: 1,
    penetration_percent: 80,
    counting_tutor: true,
    counting_quiz: true,
    bet_analyzer: true,
  });
  await expect(page.locator(".blackjack-trainer-count")).toContainText("-3");
  await expect(page.locator(".blackjack-trainer-log")).toContainText("Dealer up Th");
  await expect(page.locator(".blackjack-analysis")).toContainText("basic strategy prefers Hit");
  await expect(page.locator(".blackjack-quiz")).toContainText("What is the running count?");
  await page.getByRole("button", { name: "-3" }).click();
  await expect(page.locator(".blackjack-quiz")).toContainText("Correct");
});
