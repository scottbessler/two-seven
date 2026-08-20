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

test("coin menu repays a loan", async ({ page }) => {
  await signIn(page, "blackjackrepay");
  await page.request.post("/api/bank", { data: {} });
  await page.goto("/blackjack");

  const widget = page.locator(".bank-widget");
  const summary = widget.locator("summary");
  await summary.click();
  await expect(widget).toHaveAttribute("open", "");
  await page.keyboard.press("Escape");
  await expect(widget).not.toHaveAttribute("open", "");
  await expect(summary).toBeFocused();
  await summary.click();
  await page.locator(".blackjack-shell").click({ position: { x: 10, y: 10 } });
  await expect(widget).not.toHaveAttribute("open", "");
  await summary.click();
  await expect(page.getByRole("button", { name: "Pay back $1,000" })).toBeEnabled();
  await page.getByRole("button", { name: "Pay back $1,000" }).click();

  await expect(page.locator("#bank-balance")).toHaveText("$0");
  await expect(page.getByRole("button", { name: /Pay back/ })).toHaveCount(0);
});

test("blackjack bank mutations refresh the header balance", async ({ page }) => {
  await signIn(page, "blackjackbank");
  await page.request.post("/api/bank", { data: {} });
  await page.goto("/blackjack");
  await expect(page.locator("#bank-balance")).toHaveText("$1,000");

  await page.getByRole("button", { name: "Deal $10" }).click();

  await expect(page.locator("#bank-balance")).toHaveText("$990");
});

test("bank header delta shows net change in the last hour", async ({ page }) => {
  await signIn(page, "bankhour");
  const now = new Date();
  await page.route("**/api/bank", async (route) => {
    await route.fulfill({
      json: {
        balance: 125_000,
        loan_count: 0,
        entries: [
          { at: new Date(now.getTime() - 2 * 60 * 60 * 1000).toISOString(), delta: 50_000, memo: "old win" },
          { at: new Date(now.getTime() - 45 * 60 * 1000).toISOString(), delta: 20_000, memo: "recent win" },
          { at: new Date(now.getTime() - 10 * 60 * 1000).toISOString(), delta: -5_000, memo: "recent bet" },
        ],
      },
    });
  });

  await page.goto("/blackjack");

  await expect(page.locator("#bank-balance")).toHaveText("$1,250");
  await expect(page.locator("#bank-delta")).toHaveText(" (+$150)");
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
          penetration_percent: 50,
          counting_tutor: true,
          counting_quiz: true,
          bet_analyzer: true,
        },
        count: { running: -3, true_count: -3.3, visible_cards: 4, penetration_percent: 7 },
        trainer_log: ["Dealer up Th: -1 -> -1", "Hand 1 6s: +1 -> 0", "Dealer 7d: 0 -> -3"],
        quiz: { prompt: "What is the running count?", choices: [-4, -3, -2, -1], answer: -3 },
        analysis: ["Stand was off; basic strategy prefers Hit here."],
        shoe: { decks: 1, total_cards: 52, dealt_cards: 4, remaining_cards: 48, cut_card: 26, penetration_percent: 50, hands_dealt: 1, fresh_shuffle: true },
      },
    });
  });
  await page.goto("/blackjack");
  await page.getByRole("button", { name: "Card display settings" }).click();
  await page.locator('select[name="blackjack-decks"]').selectOption("1");
  await expect(page.locator('input[name="blackjack-penetration-percent"]')).toHaveValue("50");
  await page.locator('input[name="counting-tutor"]').check();
  await page.locator('input[name="counting-quiz"]').check();
  await page.locator('input[name="bet-analyzer"]').check();
  await page.getByRole("button", { name: "Close" }).click();

  await page.getByRole("button", { name: "Deal $10" }).click();

  expect(startPayload.settings).toEqual({
    decks: 1,
    penetration_percent: 50,
    counting_tutor: true,
    counting_quiz: true,
    bet_analyzer: true,
  });
  await expect(page.locator(".blackjack-trainer-count")).toContainText("-3");
  await expect(page.locator(".blackjack-trainer-log")).toContainText("Dealer up Th");
  await expect(page.locator(".blackjack-analysis")).toContainText("basic strategy prefers Hit");
  await expect(page.locator(".blackjack-quiz")).toContainText("What is the running count?");
  const handGeometry = await page.locator(".blackjack-play-area").evaluate((area) => {
    const areaBox = area.getBoundingClientRect();
    const [dealer, player] = [...area.querySelectorAll(".blackjack-hand")].map((hand) => hand.getBoundingClientRect());
    return {
      dealerAbovePlayer: dealer.bottom <= player.top,
      dealerCentered: Math.abs(dealer.left + dealer.width / 2 - (areaBox.left + areaBox.width / 2)),
      playerCentered: Math.abs(player.left + player.width / 2 - (areaBox.left + areaBox.width / 2)),
      dealer: { top: dealer.top, bottom: dealer.bottom },
      player: { top: player.top, bottom: player.bottom },
    };
  });
  expect(handGeometry.dealerAbovePlayer, `dealer should sit above player ${JSON.stringify(handGeometry)}`).toBe(true);
  expect(handGeometry.dealerCentered, `dealer should be centered ${JSON.stringify(handGeometry)}`).toBeLessThanOrEqual(1);
  expect(handGeometry.playerCentered, `player should be centered ${JSON.stringify(handGeometry)}`).toBeLessThanOrEqual(1);
  await page.getByRole("button", { name: "-3" }).click();
  await expect(page.locator(".blackjack-quiz")).toContainText("Correct");
});

test("blackjack gear lives in the header and the table uses stable rows", async ({ page }) => {
  await signIn(page, "blackjacklayout");
  await page.request.post("/api/bank", { data: {} });
  await page.goto("/blackjack");

  await expect(page.locator(".site-header .table-config-button")).toBeVisible();
  await expect(page.locator(".blackjack-table > .card-settings")).toHaveCount(0);
  await expect(page.locator(".header-context")).toHaveText("Blackjack");

  const geometry = await page.locator(".page").evaluate((pageRoot) => {
    const header = pageRoot.querySelector(".site-header").getBoundingClientRect();
    const settings = pageRoot.querySelector(".table-config-button").getBoundingClientRect();
    const table = pageRoot.querySelector(".blackjack-table").getBoundingClientRect();
    return {
      settingsInHeader: settings.top >= header.top && settings.bottom <= header.bottom,
      tableBelowHeader: table.top >= header.bottom,
    };
  });
  expect(geometry.settingsInHeader, "blackjack settings gear should sit in the app header").toBe(true);
  expect(geometry.tableBelowHeader, "blackjack table should start below the app header").toBe(true);
});
