import { expect, test } from "@playwright/test";

const tableState = {
  id: "mock",
  name: "Friday Night Hold'em",
  stakes: { NoLimit: { small_blind: 100, big_blind: 200 } },
  button: 5,
  viewer_seat: 0,
  tournament: null,
  last_hand: null,
  seats: [
    { index: 0, stack: 18_800, occupant: "human", display_name: "You", sitting_out: false, hole_cards: null, bank_balance: -5_000, bank_entries: [] },
    { index: 1, stack: 22_400, occupant: "Fish", display_name: "Mina", sitting_out: false, hole_cards: null, bank_balance: 14_000, bank_entries: [] },
    { index: 2, stack: 16_600, occupant: "human", display_name: "Dev", sitting_out: false, hole_cards: null, bank_balance: 8_000, bank_entries: [] },
    { index: 3, stack: 0, occupant: "Rock", display_name: "Ari", sitting_out: false, hole_cards: null, bank_balance: -2_000, bank_entries: [] },
    { index: 4, stack: 20_000, occupant: "human", display_name: "Sam", sitting_out: false, hole_cards: null, bank_balance: 25_000, bank_entries: [] },
    { index: 5, stack: 19_800, occupant: "Grinder", display_name: "Jo", sitting_out: false, hole_cards: null, bank_balance: 5_000, bank_entries: [] },
  ],
  hand: {
    street: "Flop",
    board: ["Ah", "7c", "2s"],
    your_hole_cards: ["Kh", "Qh"],
    seats: [],
    pot: 7_600,
    last_bet: 2_400,
    to_call: 1_200,
    current_player: 2,
    legal_actions: null,
    summary: null,
    players: [
      { seat: 0, contribution: 1_200, street_contribution: 0, folded: false, all_in: false, acted: true },
      { seat: 1, contribution: 2_400, street_contribution: 2_400, folded: false, all_in: false, acted: true },
      { seat: 2, contribution: 1_200, street_contribution: 1_200, folded: false, all_in: false, acted: false },
      { seat: 3, contribution: 20_000, street_contribution: 2_400, folded: false, all_in: true, acted: true },
      { seat: 4, contribution: 200, street_contribution: 0, folded: true, all_in: false, acted: true },
      { seat: 5, contribution: 200, street_contribution: 0, folded: false, all_in: false, acted: true },
    ],
    events: [
      { street: "Preflop", seat: 5, kind: "SmallBlind", amount: 100 },
      { street: "Preflop", seat: 0, kind: "BigBlind", amount: 200 },
      { street: "Preflop", seat: 1, kind: "Raise", amount: 1_200 },
      { street: "Preflop", seat: 4, kind: "Fold", amount: 0 },
      { street: "Preflop", seat: 3, kind: "AllIn", amount: 20_000 },
      { street: "Flop", seat: null, kind: "Deal", amount: 0 },
      { street: "Flop", seat: 1, kind: "Bet", amount: 2_400 },
    ],
  },
};

test("offers six concise game presets", async ({ page }) => {
  await page.goto("/tables/new");
  await expect(page.locator(".setup-option")).toHaveCount(6);
  await expect(page).toHaveScreenshot("game-setup.png", { fullPage: true });
});

test("shows live hand cues and event log", async ({ page }) => {
  await page.route("**/tables/mock/state", (route) => route.fulfill({ json: tableState }));
  await page.route("**/tables/mock/events", (route) =>
    route.fulfill({
      contentType: "text/event-stream",
      body: `event: state\ndata: ${JSON.stringify(tableState)}\n\n`,
    }),
  );
  await page.goto("/card-test");
  await page.locator("main").evaluate((main) => {
    main.innerHTML = '<div id="table-app" data-table-id="mock"></div>';
  });
  await page.evaluate(() => import(`/public/table.js?e2e=${Date.now()}`));
  await expect(page.locator(".game-log")).toBeVisible();
  await expect(page.locator(".seat.acting")).toHaveCount(1);
  await expect(page.locator(".seat-wager")).toHaveCount(3);
  await expect(page.locator(".seat.folded")).toHaveCount(1);
  await expect(page.locator(".seat.all-in")).toHaveCount(1);
  await expect(page).toHaveScreenshot("live-table.png", { fullPage: true });
});
