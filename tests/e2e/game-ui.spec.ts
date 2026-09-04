import { IPHONE_LANDSCAPE, IPHONE_MAX_PORTRAIT, IPHONE_PORTRAIT, IPHONE_SE_PORTRAIT, useDevice } from "./devices";
import { expect, test } from "./fixtures";
import { expectLayout } from "./layout";
import { expectImage } from "./rendering";

/**
 * The table's load-bearing boxes. A layout snapshot over these catches the
 * regressions a full-page image reports as "everything moved", and does it in a
 * diff a reviewer can read.
 */
const TABLE_LAYOUT = [
  ".site-header",
  ".table-stage",
  ".felt",
  ".table-center",
  ".board .playing-card",
  ".seat",
  ".seat-stack",
  ".seat-wager",
  ".seat-role",
  ".seat.viewer",
  ".seat.viewer .seat-cards .playing-card",
  ".decision-area",
  ".actions button",
  ".game-log",
  ".table-controls",
];

const tableState = {
  id: "mock",
  name: "Friday Night Hold'em",
  stakes: { NoLimit: { small_blind: 100, big_blind: 200 } },
  button: 5,
  viewer_seat: 2,
  viewer_leaving: false,
  bank_balance: 25_000,
  buy_in: 20_000,
  lends_buy_in: true,
  tournament: null,
  last_hand: null,
  seats: [
    { index: 0, stack: 18_800, occupant: "human", display_name: "Dev", sitting_out: false, hole_cards: null, bank_balance: -5_000, bank_entries: [] },
    { index: 1, stack: 22_400, occupant: "Fish", display_name: "Mina", sitting_out: false, hole_cards: null, bank_balance: 14_000, bank_entries: [] },
    { index: 2, stack: 16_600, occupant: "human", display_name: "You", sitting_out: false, hole_cards: null, bank_balance: 8_000, bank_entries: [] },
    { index: 3, stack: 0, occupant: "Rock", display_name: "Ari", sitting_out: false, hole_cards: null, bank_balance: -2_000, bank_entries: [] },
    { index: 4, stack: 20_000, occupant: "human", display_name: "Sam", sitting_out: false, hole_cards: null, bank_balance: 25_000, bank_entries: [] },
    { index: 5, stack: 19_800, occupant: "Grinder", display_name: "Jo", sitting_out: false, hole_cards: null, bank_balance: 5_000, bank_entries: [
      { memo: "Table buy-in", delta: -20_000 },
      { memo: "Cash out", delta: 18_000 },
      { memo: "Table buy-in", delta: -20_000 },
    ] },
  ],
  hand: {
    street: "Flop",
    big_blind: 200,
    board: ["Ah", "7c", "2s"],
    your_hole_cards: ["5c", "6c"],
    seats: [],
    pot: 7_600,
    last_bet: 2_400,
    to_call: 1_200,
    current_player: 2,
    legal_actions: {
      seat: 2,
      actions: ["Fold", "Call", { Raise: { amount: 2_400 } }, "AllIn"],
      to_call: 1_200,
      wager: { min: 2_400, max: 15_400, fixed: null },
    },
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

const showdownState = {
  ...tableState,
  viewer_seat: 0,
  hand: null,
  next_hand_at: "2099-01-01T00:00:00Z",
  last_hand: {
    board: ["Ah", "7c", "2s", "7d", "As"],
    results: [
      { seat: 0, hand: { label: "Two pair, aces and sevens" } },
      { seat: 1, hand: { label: "Full house, aces over sevens" } },
    ],
    awards: [{ seat: 1, amount: 40_000 }],
    contributions: { 0: 20_000, 1: 20_000 },
    revealed_hole_cards: [[0, ["Kh", "Qh"]], [1, ["Ac", "Ad"]]],
    events: [
      { street: "River", seat: 0, kind: "AllIn", amount: 18_000 },
      { street: "Complete", seat: 1, kind: "Award", amount: 40_000 },
    ],
  },
};

const foldResultState = {
  ...showdownState,
  last_hand: {
    ...showdownState.last_hand,
    board: ["Ah", "7c", "2s"],
    results: [],
    revealed_hole_cards: [],
  },
};

const tournamentCompleteRailState = {
  ...tableState,
  viewer_seat: null,
  tournament: { finished: true, finish_order: [3, 2, 1], name: "Friday Night Hold'em" },
  hand: null,
  next_hand_at: "2099-01-01T00:00:00Z",
  seats: tableState.seats.map((seat, index) => (
    index === 0 ? { ...seat, display_name: "Jaxdragambler" } : seat
  )),
  last_hand: {
    board: ["8c", "Jh", "6s", "3s", "5s"],
    results: tableState.seats.map((seat) => ({ seat: seat.index, hand: { label: "Pair of eights" } })),
    awards: [{ seat: 0, amount: 12_200 }],
    contributions: { 0: 6_100, 1: 6_100 },
    revealed_hole_cards: tableState.seats.map((seat) => [seat.index, ["8d", "Kd"]]),
    events: [
      { street: "Preflop", seat: 1, kind: "SmallBlind", amount: 100 },
      { street: "Preflop", seat: 0, kind: "BigBlind", amount: 200 },
      { street: "Complete", seat: 0, kind: "Award", amount: 12_200 },
    ],
  },
};

async function mountTable(page, state) {
  await page.unroute("**/tables/mock/state");
  await page.unroute("**/tables/mock/events");
  await page.route("**/tables/mock/state", (route) => route.fulfill({ json: state }));
  await page.route("**/tables/mock/events", (route) =>
    route.fulfill({
      contentType: "text/event-stream",
      body: `event: state\ndata: ${JSON.stringify(state)}\n\n`,
    }),
  );
  await page.goto("/card-test");
  await page.locator("main").evaluate((main) => {
    for (const child of main.querySelectorAll(":scope > :not(.site-header)")) child.remove();
    const header = main.querySelector(".site-header");
    const context = document.createElement("span");
    context.className = "header-context";
    context.textContent = "Friday Night Hold'em";
    header?.insertBefore(context, header.querySelector(".bank-widget"));
    header?.insertAdjacentHTML("beforeend", '<button class="table-config-button" type="button" title="Card display settings" aria-label="Card display settings" commandfor="card-config" command="show-modal">⚙</button>');
    main.insertAdjacentHTML("beforeend", '<div id="table-app" data-table-id="mock"></div>');
  });
  await page.evaluate(() => import(`/public/table.js?e2e=${Date.now()}`));
}

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

test("builds a tournament one question at a time", async ({ page }) => {
  await signIn(page, "setup");
  // A single re-up buys the cheaper entries and nothing above them.
  await page.request.post("/api/bank", { data: {} });
  await page.goto("/tables/new");
  await expect(page.locator("#game-setup")).toBeVisible();
  await expect(page.locator(".setup-step:not([hidden])")).toContainText("How many players?");
  // Cash games are standing tables now, so nothing here asks about them.
  await expect(page.locator('.setup-option[data-choice="betting"]')).toHaveCount(0);

  await page.locator('.setup-option[value="9"]').click();
  await expect(page.locator(".setup-step:not([hidden])")).toContainText("How much to buy in?");
  await expect(page.locator('.setup-option[value="100000"]')).toBeEnabled();
  await expect(page.locator('.setup-option[value="50000"]')).toHaveCount(0);
  await expect(page.locator('.setup-option[value="200000"]')).toHaveCount(0);
  await expect(page.locator('.setup-option[value="1000000"]')).toHaveCount(0);

  await page.locator('.setup-option[value="100000"]').click();
  await expect(page.locator("#setup-summary")).toHaveText("$1,000 tournament · 9 players · 10,000 chips · top 3 paid");
  await expect(page.locator(".setup-step:not([hidden]) legend")).toHaveText("Name");
  await expect(page.locator('input[name="name"]')).toHaveValue("Friday night");
  await page.fill('input[name="name"]', "Sunday deep");
  await page.locator(".setup-create").click();
  await page.waitForURL(/\/tables\/[0-9a-f-]+$/);
  await expect(page.locator(".tournament-panel")).toHaveCount(0);
  await expect(page.locator(".header-info")).toHaveAttribute("aria-label", /Level 1/);
  await expect(page.locator(".header-info")).toHaveAttribute("aria-label", /Blinds \$100\/\$200/);
});

test("cash table buy-ins refresh the header balance", async ({ page }) => {
  let account = {
    balance: 100_000,
    can_re_up: false,
    loan_count: 1,
    entries: [{ delta: 100_000, memo: "re-up loan" }],
  };
  await page.route("**/api/bank", async (route) => {
    if (route.request().method() === "POST") {
      account = { ...account, balance: account.balance + 100_000, can_re_up: false, entries: [...account.entries, { delta: 100_000, memo: "re-up loan" }] };
    }
    await route.fulfill({ json: account });
  });
  const unseated = {
    ...tableState,
    viewer_seat: null,
    hand: null,
    seats: tableState.seats.map((seat) => seat.index === 2
      ? { ...seat, occupant: "empty", display_name: null, stack: 0 }
      : seat),
  };
  await page.route("**/tables/mock/join", async (route) => {
    account = { ...account, balance: account.balance - tableState.buy_in, entries: [...account.entries, { delta: -tableState.buy_in, memo: "table buy-in" }] };
    unseated.viewer_seat = 2;
    unseated.seats[2] = { ...tableState.seats[2], stack: tableState.buy_in };
    await route.fulfill({ json: { ok: true } });
  });
  await mountTable(page, unseated);
  await expect(page.locator("#bank-balance")).toHaveText("$1,000");
  await expect(page.getByRole("button", { name: /Buy In/ })).toBeEnabled();
  await page.getByRole("button", { name: /Buy In/ }).click();
  await expect(page.locator(".seat.viewer")).toBeVisible();
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Leave"]);
  await expect(page.locator("#bank-balance")).toHaveText("$800");
});

test("cash table commands notice a same-page re-up", async ({ page }) => {
  let account = {
    balance: 0,
    can_re_up: true,
    loan_count: 0,
    entries: [],
  };
  await page.route("**/api/bank", async (route) => {
    if (route.request().method() === "POST") {
      account = { ...account, balance: 100_000, can_re_up: false, loan_count: 1, entries: [{ delta: 100_000, memo: "re-up loan" }] };
    }
    await route.fulfill({ json: account });
  });
  await mountTable(page, {
    ...tableState,
    viewer_seat: null,
    bank_balance: 0,
    // Nothing is lent for this seat, so it waits on the re-up.
    lends_buy_in: false,
    hand: null,
    seats: tableState.seats.map((seat) => seat.index === 2
      ? { ...seat, occupant: "empty", display_name: null, stack: 0 }
      : seat),
  });
  await expect(page.getByRole("button", { name: /Buy In/ })).toBeDisabled();

  await page.locator(".bank-widget summary").click();
  await page.locator(".re-up-button").click();

  await expect(page.getByRole("button", { name: /Buy In/ })).toBeEnabled();
});

test("re-ups from the lobby without a manual refresh", async ({ page }) => {
  await signIn(page, "reuplobby");
  await page.evaluate(() => document.documentElement.setAttribute("data-still-loaded", "yes"));
  await expect(page.locator("#bank-balance")).toHaveText("$0");
  const cheapest = page.locator("li", { hasText: "$1.00/$2.00 No-Limit" });
  // The cheap rungs lend the shortfall, so a broke player is not shut out of
  // them; the deeper games are still filed under out of reach.
  await expect(page.locator(".out-of-reach").locator(cheapest)).toHaveCount(0);
  await expect(cheapest).toHaveCount(1);
  const dear = page.locator("li", { hasText: "$10.00/$20.00 No-Limit" });
  await expect(page.locator(".out-of-reach").locator(dear)).toHaveCount(1);
  // Mark the sections that are on the page now, so a re-render is visible even
  // when the lists it produces read the same.
  await page.evaluate(() => {
    for (const section of document.querySelectorAll(".table-list")) section.setAttribute("data-stale", "yes");
  });

  await page.locator(".bank-widget summary").click();
  await page.locator(".re-up-button").click();

  await expect(page.locator("#bank-balance")).toHaveText("$1,000");
  // The lobby list is rendered by the server against your balance, so it only
  // catches up on a re-up if the page went back for a fresh copy of it.
  await expect(page.locator(".table-list[data-stale]")).toHaveCount(0);
  await expect(page.locator(".table-list")).not.toHaveCount(0);
  await expect(cheapest).toHaveCount(1);
  await expect(page.locator(".out-of-reach").locator(dear)).toHaveCount(1);
  // ...and did so without navigating: a reload would have made all of the above
  // true whether or not the re-up updated anything, and would have dropped the
  // marker with the old document.
  await expect(page.locator("html[data-still-loaded]")).toHaveCount(1);
});

test("shows live hand cues and event log", async ({ page }) => {
  await mountTable(page, tableState);
  await expect(page.locator(".game-log")).toBeVisible();
  await expect(page.locator(".game-log h2")).toHaveCount(0);
  await expect(page.locator(".table-status")).toHaveCount(0);
  await expect(page.locator(".game-log .status-log")).toContainText("Flop");
  await expect(page.locator(".game-log .status-log")).toContainText("You to act · $12 to call");
  await expect(page.locator(".seat.viewer .seat-cards .playing-card")).toHaveCount(2);
  const viewerCard = page.locator(".seat.viewer .seat-cards .playing-card").first();
  const secondViewerCard = page.locator(".seat.viewer .seat-cards .playing-card").nth(1);
  expect(await viewerCard.locator(".card-corner b").first().evaluate((rank) => getComputedStyle(rank).color)).toBe("rgb(32, 35, 31)");
  const configButtonBox = await page.getByRole("button", { name: "Card display settings" }).boundingBox();
  expect(configButtonBox.x).toBeGreaterThan((page.viewportSize()?.width || 0) * 0.75);
  expect(configButtonBox.y).toBeLessThan(80);
  if ((page.viewportSize()?.width || 0) > 760) {
    const headerBox = await page.locator(".site-header").boundingBox();
    const headerCenter = headerBox.y + headerBox.height / 2;
    const configCenter = configButtonBox.y + configButtonBox.height / 2;
    expect(Math.abs(configCenter - headerCenter), "settings button should align with the header center").toBeLessThanOrEqual(1);
  }
  await expect(page.locator(".card-config-dialog")).not.toBeVisible();
  await page.getByRole("button", { name: "Card display settings" }).click();
  await expect(page.locator(".card-config-dialog")).toBeVisible();
  const sizeSlider = page.locator('input[name="card-scale"]');
  const rankSlider = page.locator('input[name="rank-scale"]');
  const weightSlider = page.locator('input[name="rank-weight"]');
  const fourColorToggle = page.locator('input[name="four-color"]');
  await Promise.all([sizeSlider, rankSlider, weightSlider].map(async (slider) => {
    await expect(slider).toHaveValue("100");
    await expect(slider).toHaveAttribute("min", "50");
    await expect(slider).toHaveAttribute("max", "200");
  }));
  await expect(fourColorToggle).not.toBeChecked();
  await expect(page.locator(".card-config-preview .playing-card")).toHaveCount(2);
  const previewBox = await page.locator(".card-config-preview .playing-card").first().boundingBox();
  const liveBox = await viewerCard.boundingBox();
  expect(Math.abs(previewBox.width - liveBox.width)).toBeLessThan(1);
  expect(Math.abs(previewBox.height - liveBox.height)).toBeLessThan(1);
  await expectImage(page.locator(".card-config-dialog"), "card-config-dialog.png");
  // Card geometry lands in a deferred effect, so wait on the variable itself
  // rather than the slider readout it renders alongside.
  const cardWidthVariable = () => page.evaluate(() => document.documentElement.style.getPropertyValue("--viewer-card-w"));
  await sizeSlider.fill("50");
  await expect.poll(cardWidthVariable).toBe("2.7rem");
  const initialCardBox = await viewerCard.boundingBox();
  await sizeSlider.fill("100");
  await expect(page.locator(".card-config-dialog output").first()).toHaveText("100%");
  await expect.poll(cardWidthVariable).toBe("5.4rem");
  const enlargedCardBox = await viewerCard.boundingBox();
  expect(enlargedCardBox.width).toBeGreaterThan(initialCardBox.width * 1.6);
  await sizeSlider.fill("200");
  await expect.poll(cardWidthVariable).toBe("10.8rem");
  const maximumCardBox = await viewerCard.boundingBox();
  // A portrait phone clamps the top setting to the height it can actually spare,
  // so the full step only lands where the viewport can afford it.
  const heightCapped = (page.viewportSize()?.height || 0) < 900;
  expect(maximumCardBox.width).toBeGreaterThan(enlargedCardBox.width * (heightCapped ? 1.4 : 1.6));
  await sizeSlider.fill("100");
  await expect.poll(cardWidthVariable).toBe("5.4rem");
  expect(await page.evaluate(() => localStorage.getItem("table-card-size-percent"))).toBe("100");
  const rankWeightVariable = () => page.evaluate(() => document.documentElement.style.getPropertyValue("--card-rank-weight"));
  const rankStrokeVariable = () => page.evaluate(() => document.documentElement.style.getPropertyValue("--card-rank-stroke"));
  await rankSlider.fill("50");
  await weightSlider.fill("50");
  await expect.poll(rankWeightVariable).toBe("400");
  await expect.poll(rankStrokeVariable).toBe("0.000em");
  const initialRank = await viewerCard.locator(".card-corner").first().evaluate((corner) => ({ size: parseFloat(getComputedStyle(corner.querySelector("b")).fontSize), suitSize: parseFloat(getComputedStyle(corner.querySelector("i")).fontSize), weight: Number(getComputedStyle(corner.querySelector("b")).fontWeight), stroke: getComputedStyle(corner.querySelector("b")).webkitTextStrokeWidth }));
  await rankSlider.fill("100");
  await weightSlider.fill("100");
  await expect.poll(rankWeightVariable).toBe("700");
  await expect.poll(rankStrokeVariable).toBe("0.000em");
  const tunedRank = await viewerCard.locator(".card-corner").first().evaluate((corner) => ({ size: parseFloat(getComputedStyle(corner.querySelector("b")).fontSize), suitSize: parseFloat(getComputedStyle(corner.querySelector("i")).fontSize), weight: Number(getComputedStyle(corner.querySelector("b")).fontWeight), stroke: getComputedStyle(corner.querySelector("b")).webkitTextStrokeWidth }));
  expect(tunedRank.size).toBeGreaterThan(initialRank.size);
  expect(tunedRank.suitSize).toBeGreaterThan(initialRank.suitSize);
  expect(tunedRank.weight).toBeGreaterThan(initialRank.weight);
  expect(tunedRank.weight).toBe(700);
  expect(tunedRank.stroke).toBe(initialRank.stroke);
  await weightSlider.fill("200");
  await expect.poll(rankWeightVariable).toBe("900");
  await expect.poll(rankStrokeVariable).toBe("0.045em");
  const heavyRank = await viewerCard.locator(".card-corner b").first().evaluate((rank) => ({ weight: Number(getComputedStyle(rank).fontWeight), stroke: parseFloat(getComputedStyle(rank).webkitTextStrokeWidth) }));
  expect(heavyRank.weight).toBe(900);
  expect(heavyRank.stroke).toBeGreaterThan(0);
  await weightSlider.fill("100");
  expect(await page.evaluate(() => localStorage.getItem("table-rank-size-percent"))).toBe("100");
  expect(await page.evaluate(() => localStorage.getItem("table-rank-weight-percent"))).toBe("100");
  await fourColorToggle.check();
  await expect(page.locator("html")).toHaveClass(/four-color-suits/);
  await expect(viewerCard).toHaveCSS("color", "rgb(18, 79, 140)");
  expect(await page.evaluate(() => localStorage.getItem("table-four-color-suits"))).toBe("on");
  await fourColorToggle.uncheck();
  await expect(page.locator("html")).not.toHaveClass(/four-color-suits/);
  await expect(viewerCard).toHaveCSS("color", "rgb(32, 35, 31)");
  expect(await page.evaluate(() => localStorage.getItem("table-four-color-suits"))).toBe("off");
  await page.getByRole("button", { name: "Close" }).click();
  const handBeforeMagnify = await page.locator(".seat.viewer .seat-cards").boundingBox();
  const firstBeforeMagnify = await viewerCard.boundingBox();
  const secondBeforeMagnify = await secondViewerCard.boundingBox();
  // A finger has no hover, and iOS leaves :hover stuck on the last thing
  // tapped, so touch magnifies on hold instead. Hold the hand the same way.
  const hoverable = await page.evaluate(() => matchMedia("(hover: hover)").matches);
  if (hoverable) await viewerCard.hover();
  else {
    await page.mouse.move(firstBeforeMagnify.x + firstBeforeMagnify.width / 2, firstBeforeMagnify.y + firstBeforeMagnify.height / 2);
    await page.mouse.down();
  }
  await page.waitForTimeout(250);
  const handMagnified = await page.locator(".seat.viewer .seat-cards").boundingBox();
  const firstMagnified = await viewerCard.boundingBox();
  const secondMagnified = await secondViewerCard.boundingBox();
  expect(handMagnified.y, "viewer card zoom should grow upward").toBeLessThan(handBeforeMagnify.y);
  expect(Math.abs((handMagnified.y + handMagnified.height) - (handBeforeMagnify.y + handBeforeMagnify.height)), "viewer card zoom should keep its bottom anchored").toBeLessThanOrEqual(2);
  expect(firstMagnified.width).toBeGreaterThan(firstBeforeMagnify.width * 1.1);
  expect(secondMagnified.width).toBeGreaterThan(secondBeforeMagnify.width * 1.1);
  // Reaching for one card lifts the hand, not that card: hovering a single
  // card must not grow it past its partner.
  expect(
    Math.abs(firstMagnified.width / firstBeforeMagnify.width - secondMagnified.width / secondBeforeMagnify.width),
    "both hole cards should zoom by the same factor",
  ).toBeLessThan(0.02);
  if (hoverable) await page.locator(".brand").hover();
  else await page.mouse.up();
  await page.waitForTimeout(250);
  // Letting go puts the hand back exactly where it was: a magnified hand left
  // hanging over the buttons is how a tap lands on the wrong control.
  const handReleased = await page.locator(".seat.viewer .seat-cards").boundingBox();
  expect(handReleased, "the hand must not stay magnified after the touch ends").toEqual(handBeforeMagnify);
  await expect(page.locator(".empty-seat")).toHaveCount(0);
  await expect(page.locator(".board .empty-card")).toHaveCount(0);
  await expect(page.locator(".actions input")).toHaveCount(0);
  // Wager buttons name the street total they raise to, so they never read the same
  // as the call. The compact layout shares the middle column with Call, so it shows
  // two presets where the desktop shows three.
  await expect(page.locator(".actions button")).toHaveText(
    test.info().project.name === "chromium-mobile"
      ? ["Fold", "Call $12", "$68", "$112", "Raise…", "All In"]
      : ["Fold", "Call $12", "$68", "$90", "$112", "Raise…", "All In"],
  );
  await page.locator(".seat.viewer .player-info").hover();
  await expect(page.locator(".seat.viewer .player-tooltip")).toContainText("Lifetime balance");
  const topSeatIndex = await page.locator(".seat").evaluateAll((seats) => seats
    .map((seat, index) => ({ index, top: seat.getBoundingClientRect().top }))
    .toSorted((a, b) => a.top - b.top)[0].index);
  const topPlayer = page.locator(".seat").nth(topSeatIndex).locator(".player-info");
  await topPlayer.hover();
  const tooltipBox = await topPlayer.locator(".player-tooltip").boundingBox();
  const viewport = page.viewportSize();
  expect(tooltipBox.x, "V20: top-seat tooltip left edge must remain visible").toBeGreaterThanOrEqual(0);
  expect(tooltipBox.y, "V20: top-seat tooltip top edge must remain visible").toBeGreaterThanOrEqual(0);
  expect(tooltipBox.x + tooltipBox.width, "V20: top-seat tooltip right edge must remain visible").toBeLessThanOrEqual(viewport.width);
  expect(tooltipBox.y + tooltipBox.height, "V20: top-seat tooltip bottom edge must remain visible").toBeLessThanOrEqual(viewport.height);
  await page.locator(".brand").hover();
  await expect(page.locator(".seat.acting")).toHaveCount(1);
  await expect(page.locator(".seat.acting")).not.toContainText("ACT");
  await expect(page.locator(".seat.acting")).toHaveCSS("border-top-color", "rgb(217, 173, 85)");
  // Every seat keeps a wager slot so it does not resize when a bet lands.
  await expect(page.locator(".seat-wager")).toHaveCount(6);
  await expect(page.locator(".seat-wager:not(.no-wager)")).toHaveCount(3);
  await expect(page.locator(".seat.viewer .seat-wager")).toHaveText("$12");
  const checked = {
    ...tableState,
    hand: {
      ...tableState.hand,
      players: tableState.hand.players.map((player) => (player.seat === 1 ? { ...player, street_contribution: 0 } : player)),
      events: [...tableState.hand.events, { street: "Flop", seat: 1, kind: "Check", amount: 0 }],
    },
  };
  await mountTable(page, checked);
  await expect(page.locator(".seat").filter({ hasText: "Mina" }).locator(".seat-wager")).toHaveText("CHECKED");
  await expect(page.locator(".seat").filter({ hasText: "Mina" }).locator(".seat-wager")).not.toHaveClass(/no-wager/);
  await mountTable(page, tableState);
  const viewerWager = await page.locator(".seat.viewer .seat-wager").boundingBox();
  const viewerCards = await page.locator(".seat.viewer .seat-cards").boundingBox();
  const viewerName = await page.locator(".seat.viewer .player-info").boundingBox();
  // Name, stack and wager read down the right of your own hand, so they sit
  // beside the cards rather than above them -- but never underneath them.
  expect(viewerName.x, "viewer name must sit beside viewer cards").toBeGreaterThanOrEqual(viewerCards.x + viewerCards.width);
  expect(viewerWager.x, "viewer wager must sit beside viewer cards").toBeGreaterThanOrEqual(viewerCards.x + viewerCards.width);
  const wagerBehindCards = viewerWager.x < viewerCards.x + viewerCards.width
    && viewerWager.x + viewerWager.width > viewerCards.x
    && viewerWager.y < viewerCards.y + viewerCards.height
    && viewerWager.y + viewerWager.height > viewerCards.y;
  expect(wagerBehindCards, "V16: viewer cards must not cover the viewer wager").toBe(false);
  const opponentWagerLayout = await page.locator(".other-seats").evaluate((rail) => [...rail.querySelectorAll(".seat")].map((seat) => {
    const cardNode = seat.querySelector(".seat-cards,.seat-card-state");
    const wagerNode = seat.querySelector(".seat-wager:not(.no-wager)");
    const cards = cardNode?.getBoundingClientRect();
    const wager = wagerNode?.getBoundingClientRect();
    if (!cards || !wager || !wagerNode) return { ok: true };
    const overlaps = wager.left < cards.right && wager.right > cards.left && wager.top < cards.bottom && wager.bottom > cards.top;
    const topmost = document.elementFromPoint(wager.left + wager.width / 2, wager.top + wager.height / 2);
    const coveredByCard = Boolean(topmost?.closest?.(".seat-cards,.playing-card"));
    const coveredByControl = Boolean(topmost?.closest?.(".decision-area,.table-controls,.game-log"));
    return {
      name: seat.querySelector(".player-info strong")?.textContent,
      ok: !overlaps && wager.top >= cards.bottom && wager.width > 0 && wager.height > 0 && !coveredByCard && !coveredByControl,
      overlaps,
      coveredBy: topmost?.className || topmost?.tagName || null,
      coveredByCard,
      coveredByControl,
      cardsBottom: cards.bottom,
      wagerTop: wager.top,
      wagerBottom: wager.bottom,
    };
  }));
  expect(opponentWagerLayout.every((seat) => seat.ok), `V44: opponent wagers must sit below cards/folded state and remain visible ${JSON.stringify(opponentWagerLayout)}`).toBe(true);
  await expect(page.locator(".table-stage > .card-settings")).toHaveCount(0);
  const stageStart = await page.locator(".table-stage").evaluate((stage) => {
    const first = stage.firstElementChild;
    return {
      firstClass: first?.className || "",
      gap: first ? Math.round(first.getBoundingClientRect().top - stage.getBoundingClientRect().top) : null,
    };
  });
  expect(stageStart.firstClass).toContain("other-seats");
  expect(stageStart.gap, "table-stage must not reserve an empty first row").toBeLessThanOrEqual(1);
  await expect(page.locator(".seat.folded")).toHaveCount(1);
  await expect(page.locator(".seat.all-in")).toHaveCount(1);
  await expect(page.locator(".seat.all-in .seat-wager")).toHaveText("ALL IN");
  expect(await page.locator(".decision-area").evaluate((actions) => actions.compareDocumentPosition(document.querySelector(".game-log")) & Node.DOCUMENT_POSITION_FOLLOWING)).toBeTruthy();
  const overlaps = await page.locator(".table-stage").evaluate((stage) => {
    const cards = [...stage.querySelectorAll(".board .playing-card")].map((node) => node.getBoundingClientRect());
    const players = [...stage.querySelectorAll(".seat, .seat-cards")].map((node) => node.getBoundingClientRect());
    return players.filter((player) => cards.some((card) => player.left < card.right && player.right > card.left && player.top < card.bottom && player.bottom > card.top)).length;
  });
  expect(overlaps, "V14: players and attached cards must not overlap the board").toBe(0);
  const metricOverlaps = await page.locator(".table-stage").evaluate((stage) => {
    const metrics = stage.querySelector(".table-metrics")?.getBoundingClientRect();
    if (!metrics) return 0;
    return [...stage.querySelectorAll(".seat-cards")].filter((node) => {
      const cards = node.getBoundingClientRect();
      return cards.left < metrics.right && cards.right > metrics.left && cards.top < metrics.bottom && cards.bottom > metrics.top;
    }).length;
  });
  expect(metricOverlaps, "V17: table metrics must not overlap player cards").toBe(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
  // The shell is locked to the viewport at every size, so the page never scrolls.
  expect(await page.evaluate(() => document.documentElement.scrollHeight <= document.documentElement.clientHeight), "V23: the table must not scroll the page").toBe(true);
  expect(await page.locator(".table-shell").innerText()).not.toMatch(/\$-?\d+\.\d{2}/);
  if ((page.viewportSize()?.width || 0) > 640) {
    const geometry = await page.locator(".table-stage").evaluate((stage) => ({
      stageHeight: stage.getBoundingClientRect().height,
      viewport: document.documentElement.clientHeight,
      radius: getComputedStyle(stage.querySelector(".felt")).borderRadius,
      boardViewerGap: stage.querySelector(".seat.viewer").getBoundingClientRect().top
        - Math.max(...[...stage.querySelectorAll(".board .playing-card")].map((card) => card.getBoundingClientRect().bottom)),
    }));
    expect(geometry.stageHeight).toBeLessThanOrEqual(geometry.viewport);
    expect(geometry.boardViewerGap, "V24: board cards must keep a visible row gap above the viewer seat").toBeGreaterThanOrEqual(12);
    expect(geometry.radius).not.toContain("%");
  }
  await expectLayout(page, "live-table", TABLE_LAYOUT);
  await expectImage(page, "live-table.png", { fullPage: true });
});

test("counts down the player on the clock without moving the table", async ({ page }) => {
  // A table one person has to themselves runs no clock, and draws none.
  await mountTable(page, tableState);
  await expect(page.locator(".seat.acting")).toHaveCount(1);
  await expect(page.locator(".turn-clock")).toHaveCount(0);
  const stageBefore = await page.locator(".table-stage").boundingBox();
  const actionsBefore = await page.locator(".actions").boundingBox();

  await mountTable(page, {
    ...tableState,
    turn_seconds: 10,
    turn_deadline: new Date(Date.now() + 9_000).toISOString(),
  });
  // The seat that is to act carries the clock, and so do the viewer's own
  // buttons, since the viewer is the one being timed here.
  await expect(page.locator(".seat.acting .seat-clock")).toHaveCount(1);
  await expect(page.getByRole("timer")).toHaveAttribute("aria-label", /^\d+s to act$/);

  // It drains, and nothing it is drawn over moves while it does.
  const filled = page.locator(".seat.acting .seat-clock > i");
  const width = () => filled.evaluate((bar) => bar.getBoundingClientRect().width);
  const before = await width();
  expect(before).toBeGreaterThan(0);
  await page.waitForTimeout(1_500);
  expect(await width()).toBeLessThan(before);
  expect(await page.locator(".table-stage").boundingBox()).toEqual(stageBefore);
  expect(await page.locator(".actions").boundingBox()).toEqual(actionsBefore);
});

test("hides your own cards until you reach for them in paranoid mode", async ({ page }) => {
  await mountTable(page, tableState);
  const viewerCard = page.locator(".seat.viewer .seat-cards .playing-card").first();
  const rank = viewerCard.locator(".card-corner b");
  await expect(rank).toBeVisible();

  await page.getByRole("button", { name: "Card display settings" }).click();
  const toggle = page.locator('input[name="paranoid"]');
  await expect(toggle).not.toBeChecked();
  const preview = page.locator(".card-config-preview .playing-card .card-corner b").first();
  await expect(preview).toBeVisible();
  // The checkbox must be big enough to see and hit.
  const box = await toggle.boundingBox();
  expect(box.width, "the paranoid checkbox must be visible").toBeGreaterThan(12);
  expect(box.height).toBeGreaterThan(12);
  await toggle.check();
  // Turning it on shows itself: the dialog's own preview goes face down.
  await expect(preview).toBeHidden();
  await page.getByRole("button", { name: "Close" }).click();
  await expect(page.locator(".table-shell")).toHaveClass(/paranoid-cards/);
  // Face down: the rank is still in the DOM but concealed.
  await expect(rank).toBeHidden();
  await expect(page.locator(".seat:not(.viewer) .seat-cards .playing-card").first()).toBeVisible();

  // Hovering the viewer's cards turns them back over.
  await page.locator(".seat.viewer .seat-cards").hover();
  await expect(rank).toBeVisible();
  await page.locator(".brand").hover();
  await expect(rank).toBeHidden();

  // Touch reveals through focus, which survives until you look away.
  await viewerCard.focus();
  await expect(rank).toBeVisible();
  await page.locator(".brand").focus();
  await expect(rank).toBeHidden();

  expect(await page.evaluate(() => localStorage.getItem("table-paranoid-cards"))).toBe("on");
});

test("keeps desktop action buttons in one row at narrow widths", async ({ page }) => {
  await page.setViewportSize({ width: 702, height: 900 });
  await mountTable(page, tableState);

  const buttons = page.locator(".actions button");
  await expect(buttons).toHaveText(["Fold", "Call $12", "$68", "$90", "$112", "Raise…", "All In"]);
  const layout = await buttons.evaluateAll((nodes) => {
    const area = nodes[0].closest(".decision-area")?.getBoundingClientRect();
    const bar = nodes[0].closest(".actions")?.getBoundingClientRect();
    return nodes.map((node) => {
      const rect = node.getBoundingClientRect();
      return {
        left: Math.round(rect.left),
        top: Math.round(rect.top),
        height: Math.round(rect.height),
        bottom: rect.bottom,
        right: rect.right,
        scrollWidth: node.scrollWidth,
        clientWidth: node.clientWidth,
        scrollHeight: node.scrollHeight,
        clientHeight: node.clientHeight,
        insideActionBar: area ? rect.left >= area.left - 1 && rect.right <= area.right + 1 && rect.top >= area.top - 1 && rect.bottom <= area.bottom + 1 : false,
        barLeft: bar?.left,
        barRight: bar?.right,
      };
    });
  });
  expect(new Set(layout.map((button) => button.top)).size, "narrow desktop actions must not wrap").toBe(1);
  expect(new Set(layout.map((button) => button.height)).size, "wrapped action labels must not change individual button heights").toBe(1);
  expect(layout.every((button) => button.insideActionBar), "action buttons must stay inside the action bar").toBe(true);
  expect(layout.every((button) => button.scrollWidth <= button.clientWidth && button.scrollHeight <= button.clientHeight), "action labels must fit inside their buttons").toBe(true);
  expect(layout.map((button) => button.left)).toEqual([...layout].map((button) => button.left).toSorted((a, b) => a - b));
  expect(layout[0].left, "V44: first action should start at the action bar edge").toBeLessThanOrEqual(Math.ceil(layout[0].barLeft + 1));
  expect(layout.at(-1).right, "V44: last action should reach the action bar edge").toBeGreaterThanOrEqual(Math.floor(layout.at(-1).barRight - 1));
  const slots = await page.locator(".actions").evaluate((bar) => {
    const barBox = bar.getBoundingClientRect();
    // The right edge can carry a second action (the custom raise), so the
    // invariant is per action, not per slot.
    const edges = [...bar.querySelectorAll(".action-edge button")].map((button) => button.getBoundingClientRect().width);
    const middle = [...bar.querySelectorAll(".action-middle button")].map((button) => button.getBoundingClientRect().width);
    return { barWidth: barBox.width, edges, middle };
  });
  expect(slots.edges.every((width) => width <= slots.barWidth / 7), `V47: edge actions must be at most 1/7 of the bar ${JSON.stringify(slots)}`).toBe(true);
  expect(Math.max(...slots.middle) - Math.min(...slots.middle), "V47: middle actions must split their region evenly").toBeLessThanOrEqual(1);
});

test("keeps mobile controls uniform and hold actions tappable", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  const allInCallState = {
    ...tableState,
    seats: tableState.seats.map((seat) => (seat.index === tableState.viewer_seat ? { ...seat, stack: 1_200 } : seat)),
    hand: {
      ...tableState.hand,
      legal_actions: {
        ...tableState.hand.legal_actions,
        actions: ["Fold", "Call", "AllIn"],
        wager: null,
      },
    },
  };
  await page.goto("/card-test");
  await page.evaluate(() => localStorage.setItem("table-confirm-all-in", "on"));
  await mountTable(page, allInCallState);
  await expect(page.locator(".actions")).toBeVisible();

  const actionLayout = await page.locator(".actions button").evaluateAll((buttons) => buttons.flatMap((button) => {
    const buttonBox = button.getBoundingClientRect();
    if (!buttonBox.width || !buttonBox.height) return [];
    const range = document.createRange();
    range.selectNodeContents(button);
    const labelBox = range.getBoundingClientRect();
    const style = getComputedStyle(button);
    const lineHeight = Number.parseFloat(style.lineHeight);
    return {
      height: buttonBox.height,
      inside: labelBox.left >= buttonBox.left - 1
        && labelBox.right <= buttonBox.right + 1
        && labelBox.top >= buttonBox.top - 1
        && labelBox.bottom <= buttonBox.bottom + 1,
      fits: button.scrollWidth <= button.clientWidth,
      lineCount: labelBox.height / lineHeight,
      tapHeight: buttonBox.height,
    };
  }));
  expect(actionLayout.length).toBeGreaterThan(0);
  expect(new Set(actionLayout.map((button) => Math.round(button.height))).size, "mobile action controls must share one height").toBe(1);
  expect(actionLayout.every((button) => button.inside && button.fits && button.lineCount < 2), `V46: mobile action labels must fit ${JSON.stringify(actionLayout)}`).toBe(true);
  expect(actionLayout.every((button) => button.tapHeight >= 40), "mobile actions must retain a sane tap height").toBe(true);

  await expect(page.locator(".actions button")).toHaveText(["Fold", "All In"]);
  const allIn = page.getByRole("button", { name: "Hold All In for 1 second" });
  await expect(allIn).toHaveClass(/hold-action/);
  await expect(allIn).toHaveAttribute("title", "Hold for 1 second");
});

test("holds an all-in call in the pinned all-in slot", async ({ page }) => {
  const allInCallState = {
    ...tableState,
    seats: tableState.seats.map((seat) => (seat.index === tableState.viewer_seat ? { ...seat, stack: 1_200 } : seat)),
    hand: {
      ...tableState.hand,
      legal_actions: {
        ...tableState.hand.legal_actions,
        actions: ["Fold", "Call", "AllIn"],
        wager: null,
      },
    },
  };
  const posts = [];
  await page.route("**/tables/mock/action", async (route) => {
    posts.push(route.request().postDataJSON());
    await route.fulfill({ json: { ok: true } });
  });
  await page.goto("/card-test");
  await page.evaluate(() => localStorage.setItem("table-confirm-all-in", "on"));
  await mountTable(page, allInCallState);

  await expect(page.locator(".actions button")).toHaveText(["Fold", "All In"]);
  await expect(page.getByRole("button", { name: /Call/ })).toHaveCount(0);
  const allIn = page.getByRole("button", { name: "Hold All In for 1 second" });
  await allIn.click();
  await page.waitForTimeout(100);
  expect(posts, "V34: a tap must not submit an all-in call").toEqual([]);
  await allIn.dispatchEvent("pointerdown", { pointerId: 1, pointerType: "touch", isPrimary: true });
  await expect(allIn).toHaveClass(/holding/);
  await page.waitForTimeout(1_100);
  await expect.poll(() => posts).toEqual([{ kind: "call" }]);
});

test("shows an action as sent and refuses a second click while it flies", async ({ page }) => {
  const posts = [];
  let release;
  await page.route("**/tables/mock/action", async (route) => {
    posts.push(route.request().postDataJSON());
    await new Promise((resolve) => { release = resolve; });
    await route.fulfill({ json: { ok: true } });
  });
  await page.goto("/card-test");
  await mountTable(page, tableState);

  const call = page.getByRole("button", { name: /^Call/ });
  await call.click();
  // The pressed control says so, and every action in the row -- including a
  // second Call -- stops taking clicks until the answer lands.
  await expect(call).toHaveClass(/pending/);
  await expect(call).toHaveAttribute("aria-busy", "true");
  await expect(call).toBeDisabled();
  await expect(page.getByRole("button", { name: "Fold" })).toBeDisabled();
  await call.dispatchEvent("click");
  await page.waitForTimeout(100);
  expect(posts, "V34: a laggy network must not turn one call into two").toHaveLength(1);
  release();
  await expect(call).not.toHaveClass(/pending/);
});

test("holds a protected fold for one second", async ({ page }) => {
  const posts = [];
  await page.route("**/tables/mock/action", async (route) => {
    posts.push(route.request().postDataJSON());
    await route.fulfill({ json: { ok: true } });
  });
  await page.goto("/card-test");
  await page.evaluate(() => localStorage.setItem("table-confirm-fold", "on"));
  await mountTable(page, tableState);

  const fold = page.getByRole("button", { name: "Hold Fold for 1 second" });
  await fold.click();
  await page.waitForTimeout(100);
  expect(posts, "V34: a tap must not submit a protected fold").toEqual([]);
  await fold.dispatchEvent("pointerdown", { pointerId: 1, pointerType: "touch", isPrimary: true });
  await page.waitForTimeout(1_100);
  await expect.poll(() => posts).toEqual([{ kind: "fold" }]);
});

test("anchors narrow metrics and keeps the result action bar steady", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/card-test");
  await page.evaluate(() => localStorage.setItem("table-confirm-fold", "on"));
  await mountTable(page, tableState);
  const live = await page.locator(".table-shell").evaluate((shell) => {
    const stageNode = shell.querySelector(".table-stage");
    const stage = stageNode.getBoundingClientRect();
    const metrics = shell.querySelector(".table-metrics").getBoundingClientRect();
    const fold = shell.querySelector(".fold-action");
    return { stageLeft: stage.left, metricsLeft: metrics.left, userSelect: getComputedStyle(fold).userSelect };
  });
  expect(live.metricsLeft, "V53: narrow metrics must anchor at the table left edge").toBeLessThanOrEqual(live.stageLeft + 1);
  expect(live.userSelect, "V53: protected hold controls must suppress native text selection").toBe("none");

  const pendingResult = {
    ...foldResultState,
    last_hand: {
      ...foldResultState.last_hand,
      awards: [],
      results: [],
    },
  };
  await mountTable(page, pendingResult);
  const pendingGeometry = await page.locator(".table-shell").evaluate((shell) => {
    const decision = shell.querySelector(".decision-area").getBoundingClientRect();
    const stageNode = shell.querySelector(".table-stage");
    const stage = stageNode.getBoundingClientRect();
    return { decisionTop: decision.top, stageHeight: stage.height };
  });

  await mountTable(page, foldResultState);
  const resultGeometry = await page.locator(".table-shell").evaluate((shell) => {
    const decision = shell.querySelector(".decision-area").getBoundingClientRect();
    const stageNode = shell.querySelector(".table-stage");
    const stage = stageNode.getBoundingClientRect();
    return { decisionTop: decision.top, stageHeight: stage.height };
  });
  expect(Math.abs(resultGeometry.decisionTop - pendingGeometry.decisionTop), `V53: result state must not move mobile action bar ${JSON.stringify({ pendingGeometry, resultGeometry })}`).toBeLessThanOrEqual(1);
});

test("uses the full action bar when only a few actions are available", async ({ page }) => {
  const shortActionState = {
    ...tableState,
    hand: {
      ...tableState.hand,
      legal_actions: {
        ...tableState.hand.legal_actions,
        actions: ["Fold", "Call", "AllIn"],
        wager: null,
      },
    },
  };
  await mountTable(page, shortActionState);
  const geometry = await page.locator(".actions").evaluate((bar) => {
    const barBox = bar.getBoundingClientRect();
    const buttons = [...bar.querySelectorAll("button")].map((button) => button.getBoundingClientRect());
    return {
      count: buttons.length,
      firstLeft: buttons[0].left,
      lastRight: buttons.at(-1).right,
      barLeft: barBox.left,
      barRight: barBox.right,
      firstWidth: buttons[0].width,
      middleWidth: buttons[1].width,
      lastWidth: buttons.at(-1).width,
    };
  });
  expect(geometry.count).toBe(3);
  expect(geometry.firstLeft, `V44: short action bar should start full-width ${JSON.stringify(geometry)}`).toBeLessThanOrEqual(geometry.barLeft + 1);
  expect(geometry.lastRight, `V44: short action bar should end full-width ${JSON.stringify(geometry)}`).toBeGreaterThanOrEqual(geometry.barRight - 1);
  expect(Math.abs(geometry.firstWidth - geometry.lastWidth), "V47: Fold and All In should retain equal edge slots").toBeLessThanOrEqual(1);
  expect(geometry.middleWidth, "V47: a lone middle action should consume the flexible region").toBeGreaterThan(geometry.firstWidth * 4);
});

test("offers only fold and call once a shove caps the pot", async ({ page }) => {
  // Heads up against a shorter shove: the caller keeps chips behind, so this is
  // a call rather than an all in, and there is nobody left to raise.
  const cappedPotState = {
    ...tableState,
    hand: {
      ...tableState.hand,
      to_call: 1_200,
      legal_actions: { seat: 2, actions: ["Fold", "Call"], to_call: 1_200, wager: null },
    },
  };
  await mountTable(page, cappedPotState);
  await expect(page.locator(".actions .fold-action")).toBeVisible();
  const labels = await page.locator(".actions button").allInnerTexts();
  expect(labels.map((label) => label.replaceAll(/\s+/g, " ").trim()), "V47: a capped pot leaves one call and no wagers").toEqual([
    "Fold",
    "Call $12",
  ]);
  // It takes the All In slot and colour, but it still says what it is.
  const closing = page.locator(".actions .action-edge-right button");
  await expect(closing).toHaveClass(/all-in-action/);
  await expect(closing).toHaveAttribute("aria-label", "Call $12");
  await expect(closing).not.toHaveClass(/hold-action/);
  expect(await page.locator(".actions .action-middle button").count(), "V47: nothing is left in the middle").toBe(0);
  const slots = await page.locator(".actions").evaluate((bar) => {
    const fold = bar.querySelector(".action-edge-left button").getBoundingClientRect();
    const call = bar.querySelector(".action-edge-right button").getBoundingClientRect();
    return { barWidth: bar.getBoundingClientRect().width, foldWidth: fold.width, callWidth: call.width, gap: call.left - fold.right };
  });
  expect(slots.foldWidth, `V47: Fold keeps its narrow edge slot ${JSON.stringify(slots)}`).toBeLessThanOrEqual(slots.barWidth / 7 + 1);
  expect(slots.gap, `V47: a dead zone still separates fold from the closing call ${JSON.stringify(slots)}`).toBeGreaterThan(slots.foldWidth);
  await expectLayout(page, "capped-call-actions", TABLE_LAYOUT);
  await expectImage(page, "capped-call-actions.png", { fullPage: true });
});

test("keeps fixed-limit capped actions on one aligned row", async ({ page }) => {
  const cappedActionState = {
    ...tableState,
    hand: {
      ...tableState.hand,
      legal_actions: {
        ...tableState.hand.legal_actions,
        actions: ["Fold", "Call"],
        wager: null,
        wagers_capped: true,
      },
    },
  };
  await mountTable(page, cappedActionState);

  await expect(page.locator(".actions button")).toHaveText(["Fold", "Call $12"]);
  await expect(page.locator(".capped-note")).toHaveCount(0);
  const geometry = await page.locator(".actions button").evaluateAll((buttons) => buttons.map((button) => {
    const box = button.getBoundingClientRect();
    return { top: box.top, bottom: box.bottom, height: box.height };
  }));
  expect(new Set(geometry.map(({ top }) => Math.round(top))).size, `V51: capped actions must share a row ${JSON.stringify(geometry)}`).toBe(1);
  expect(new Set(geometry.map(({ bottom }) => Math.round(bottom))).size, `V51: capped actions must align ${JSON.stringify(geometry)}`).toBe(1);
  expect(new Set(geometry.map(({ height }) => Math.round(height))).size).toBe(1);
});

test("keeps compact table header rows from overlapping", async ({ page }) => {
  await page.setViewportSize({ width: 702, height: 900 });
  await mountTable(page, tableState);
  await page.locator(".header-context").evaluate((context) => {
    context.firstChild.textContent = "Friday Night Hold'em With A Needlessly Long Table Name";
  });
  await page.locator(".table-shell").waitFor();
  await page.getByRole("button", { name: "Card display settings" }).waitFor();

  const geometry = await page.locator(".site-header").evaluate((header) => {
    const brand = header.querySelector(".brand").getBoundingClientRect();
    const context = header.querySelector(".header-context").getBoundingClientRect();
    const bank = header.querySelector(".bank-widget").getBoundingClientRect();
    const settings = document.querySelector(".table-config-button").getBoundingClientRect();
    // Browser-evaluated helpers cannot close over test-scope functions.
    // oxlint-disable-next-line unicorn/consistent-function-scoping
    const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    return {
      contextBelowBrand: context.top > brand.bottom,
      contextOverlapsBrand: overlaps(context, brand),
      contextOverlapsBank: overlaps(context, bank),
      contextOverlapsSettings: overlaps(context, settings),
      rightControlCenterDelta: Math.abs((settings.top + settings.height / 2) - (bank.top + bank.height / 2)),
      rowGap: context.top - brand.bottom,
      // Full-bleed rendering hands the header the notch; the band that matters is
      // what it adds below that inset.
      headerHeight: header.getBoundingClientRect().height
        - Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--safe-top")),
    };
  });
  expect(geometry.contextBelowBrand, "compact table name should get its own header row").toBe(true);
  expect(geometry.contextOverlapsBrand, "compact table name must not overlap the brand").toBe(false);
  expect(geometry.contextOverlapsBank, "compact table name must not overlap the bank").toBe(false);
  expect(geometry.contextOverlapsSettings, "compact table name must not overlap settings").toBe(false);
  expect(geometry.rightControlCenterDelta, "compact header right controls should align with each other").toBeLessThanOrEqual(4);
  expect(geometry.rowGap, "compact header rows should stay tight").toBeLessThanOrEqual(3);
  expect(geometry.headerHeight, "compact header should not reserve a tall blank band").toBeLessThanOrEqual(42);
});

test("keeps a player tooltip inside a narrow desktop viewport", async ({ page }) => {
  await page.setViewportSize({ width: 702, height: 832 });
  await mountTable(page, tableState);
  const playerInfo = page.locator(".other-seats .seat .player-info").filter({ hasText: "Sam" });
  await expect(playerInfo).toHaveCount(1);
  await playerInfo.hover();
  const tooltipBox = await playerInfo.locator(".player-tooltip").boundingBox();
  expect(tooltipBox.x, "V20: narrow player tooltip left edge must remain visible").toBeGreaterThanOrEqual(0);
  expect(tooltipBox.y, "V20: narrow player tooltip top edge must remain visible").toBeGreaterThanOrEqual(0);
  expect(tooltipBox.x + tooltipBox.width, "V20: narrow player tooltip right edge must remain visible").toBeLessThanOrEqual(702);
  expect(tooltipBox.y + tooltipBox.height, "V20: narrow player tooltip bottom edge must remain visible").toBeLessThanOrEqual(832);
});

test("opens a seated player's page from their name", async ({ page }) => {
  const seated = "3f1d1c9e-6a2b-4f3c-8d5e-1a2b3c4d5e6f";
  await mountTable(page, {
    ...tableState,
    seats: tableState.seats.map((seat) => (seat.display_name === "Sam" ? { ...seat, user_id: seated } : seat)),
  });
  const name = page.locator(".seat .player-info .player-link").filter({ hasText: "Sam" });
  await expect(name).toHaveAttribute("href", `/player/${seated}`);
  // A house regular has no page to open.
  await expect(page.locator(".seat .player-info").filter({ hasText: "Mina" }).locator("a")).toHaveCount(0);
});

test("keeps compact portrait opponent seats visible", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const inspectRail = async (state, expectOutcome = false) => {
    await mountTable(page, state);
    await expect(page.locator(".other-seats .seat").first()).toBeVisible();
    const shortName = page.locator(".other-seats .player-info > strong").filter({ hasText: /^Sam$/ });
    await expect(shortName).toHaveCount(1);
    expect(
      await shortName.evaluate((node) => node.scrollWidth <= node.clientWidth),
      "compact portrait opponent names must not truncate",
    ).toBe(true);
    const geometry = await page.locator(".table-stage").evaluate((stage) => {
      const stageBox = stage.getBoundingClientRect();
      // Browser-evaluated helpers cannot close over test-scope functions.
      // oxlint-disable-next-line unicorn/consistent-function-scoping
      const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
      const seats = [...stage.querySelectorAll(".other-seats .seat")];
      return seats.map((seat) => {
        const seatBox = seat.getBoundingClientRect();
        const wager = seat.querySelector(".seat-wager");
        const wagerBox = wager?.getBoundingClientRect();
        const cornerBadges = seat.querySelector(".seat-corner-badges");
        const cornerBox = cornerBadges?.getBoundingClientRect();
        const heading = seat.querySelector(".opponent-heading")?.getBoundingClientRect();
        const playerInfo = seat.querySelector(".player-info")?.getBoundingClientRect();
        const hasCornerBadge = Boolean(cornerBadges && cornerBadges.textContent.trim());
        const outcome = seat.querySelector(".seat-outcome-badges i");
        const outcomeBox = outcome?.getBoundingClientRect();
        return {
          scrolls: seat.scrollHeight > seat.clientHeight,
          wagerVisible: wager && getComputedStyle(wager).visibility !== "hidden",
          wagerInside: Boolean(wagerBox)
            && wagerBox.top >= seatBox.top - 1
            && wagerBox.bottom <= seatBox.bottom + 1,
          hasCornerBadge,
          headingCentered: Boolean(heading)
            && Math.abs((heading.left + heading.width / 2) - (seatBox.left + seatBox.width / 2)) <= 1,
          badgeSharesNameRow: Boolean(playerInfo && cornerBox && cornerBadges.textContent.trim())
            && cornerBox.top < playerInfo.bottom
            && cornerBox.bottom > playerInfo.top,
          nameOverCorner: Boolean(playerInfo && cornerBox && cornerBadges.textContent.trim())
            && overlaps(playerInfo, cornerBox),
          outcomeVisible: Boolean(outcome && outcomeBox && outcomeBox.width > 0 && outcomeBox.height > 0),
          outcomeInsideSeat: Boolean(outcomeBox)
            && outcomeBox.top >= seatBox.top - 1
            && outcomeBox.bottom <= seatBox.bottom + 1,
          outcomeInsideStage: Boolean(outcomeBox)
            && outcomeBox.top >= stageBox.top - 1
            && outcomeBox.bottom <= stageBox.bottom + 1,
        };
      });
    });
    expect(geometry.every((seat) => !seat.scrolls), `V45: compact seats must not scroll ${JSON.stringify(geometry)}`).toBe(true);
    expect(
      geometry.filter((seat) => seat.wagerVisible).every((seat) => seat.wagerInside),
      `V45: visible opponent wagers must stay inside seats ${JSON.stringify(geometry)}`,
    ).toBe(true);
    expect(geometry.every((seat) => !seat.nameOverCorner), `V45: player names must clear corner badges ${JSON.stringify(geometry)}`).toBe(true);
    const badgeSeats = geometry.filter((seat) => seat.hasCornerBadge);
    expect(badgeSeats.length).toBeGreaterThan(0);
    expect(badgeSeats.every((seat) => seat.headingCentered), `V61: opponent identity + role groups must stay centered ${JSON.stringify(geometry)}`).toBe(true);
    expect(badgeSeats.every((seat) => seat.badgeSharesNameRow), `V45: corner badges must not buy their own row ${JSON.stringify(geometry)}`).toBe(true);
    if (expectOutcome) {
      const outcomes = geometry.filter((seat) => seat.outcomeVisible);
      expect(outcomes.length).toBeGreaterThan(0);
      expect(outcomes.every((seat) => seat.outcomeInsideSeat && seat.outcomeInsideStage), `V45: outcome badges must remain visible ${JSON.stringify(geometry)}`).toBe(true);
    }
  };

  await inspectRail(tableState);
  await inspectRail(tournamentCompleteRailState, true);
  await mountTable(page, { ...tableState, button: tableState.viewer_seat });
  const viewerBadgeLayout = await page.locator(".seat.viewer").evaluate((seat) => {
    const badges = seat.querySelector(".seat-corner-badges");
    const playerInfo = seat.querySelector(".player-info");
    const styles = getComputedStyle(playerInfo);
    return {
      hasCornerBadge: Boolean(badges && badges.textContent.trim()),
      maxWidth: styles.maxWidth,
      justifySelf: styles.justifySelf,
    };
  });
  expect(viewerBadgeLayout.hasCornerBadge).toBe(true);
  expect(viewerBadgeLayout.maxWidth).not.toBe("100%");
  expect(viewerBadgeLayout.justifySelf).toBe("start");
});

for (const viewport of [
  { width: 1440, height: 900 },
  { width: 834, height: 1112 },
  { width: 393, height: 852 },
  { width: 852, height: 393 },
]) {
  test(`keeps redesigned opponent tiles coherent at ${viewport.width}x${viewport.height}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await mountTable(page, { ...tableState, hand: { ...tableState.hand, current_player: 1 } });
    const geometry = await page.locator(".other-seats").evaluate((rail) => [...rail.querySelectorAll(".seat")].map((seat) => {
      const box = seat.getBoundingClientRect();
      const heading = seat.querySelector(".opponent-heading")?.getBoundingClientRect();
      const stack = seat.querySelector(".seat-stack")?.getBoundingClientRect();
      const cards = seat.querySelector(".seat-cards")?.getBoundingClientRect();
      const wager = seat.querySelector(".seat-wager")?.getBoundingClientRect();
      const cardTransforms = [...seat.querySelectorAll(".playing-card")].map((card) => getComputedStyle(card).transform);
      const style = getComputedStyle(seat);
      return {
        className: seat.className,
        background: style.backgroundColor,
        border: style.borderTopColor,
        clips: seat.scrollHeight > seat.clientHeight || seat.scrollWidth > seat.clientWidth,
        headingCentered: Boolean(heading) && Math.abs((heading.left + heading.width / 2) - (box.left + box.width / 2)) <= 1,
        ordered: Boolean(heading && stack && cards && wager)
          && heading.bottom <= stack.top + 1
          && stack.bottom <= cards.top + 1
          && cards.bottom <= wager.top + 1,
        rows: heading && stack && cards && wager ? {
          heading: [heading.top, heading.bottom],
          stack: [stack.top, stack.bottom],
          cards: [cards.top, cards.bottom],
          wager: [wager.top, wager.bottom],
        } : null,
        cardTransforms,
      };
    }));
    expect(geometry.every((seat) => seat.background === "rgb(16, 36, 31)"), `V61: opponent tiles use one dark surface at ${JSON.stringify(viewport)} ${JSON.stringify(geometry)}`).toBe(true);
    expect(geometry.every((seat) => !seat.clips && seat.headingCentered && seat.ordered), `V61: opponent rows remain stable at ${JSON.stringify(viewport)} ${JSON.stringify(geometry)}`).toBe(true);
    expect(geometry.flatMap((seat) => seat.cardTransforms).every((transform) => transform === "none"), `V61: opponent cards stay straight at ${JSON.stringify(viewport)} ${JSON.stringify(geometry)}`).toBe(true);
    const acting = geometry.find((seat) => seat.className.includes("acting"));
    expect(acting?.border, `V61: acting state remains distinct at ${JSON.stringify(viewport)}`).toBe("rgb(217, 173, 85)");
  });
}

test("keeps seats clear of the board in a short desktop window", async ({ page }) => {
  // Everyone's cards face up is the worst case for collisions.
  const showdownFour = {
    ...showdownState,
    viewer_seat: null,
    seats: showdownState.seats.map((seat, index) =>
      index < 4 ? seat : { ...seat, occupant: "empty", display_name: null }),
    last_hand: {
      ...showdownState.last_hand,
      revealed_hole_cards: [[0, ["Kh", "Qh"]], [1, ["Ac", "Ad"]], [2, ["7h", "Td"]], [3, ["2c", "4s"]]],
    },
  };
  // Each pass resizes and re-mounts, so these have to run in order.
  // oxlint-disable-next-line no-await-in-loop
  for (const height of [700, 900, 1200]) {
    /* oxlint-disable no-await-in-loop */
    await page.setViewportSize({ width: 1000, height });
    await mountTable(page, showdownFour);
    await expect(page.locator(".seat")).toHaveCount(4);
    const geometry = await page.locator(".table-stage").evaluate((stage) => {
      // Browser-evaluated helpers cannot close over test-scope functions.
      // oxlint-disable-next-line unicorn/consistent-function-scoping
      const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
      const middle = [...stage.querySelectorAll(".board .playing-card, .table-metrics, .showdown-result")]
        .map((node) => node.getBoundingClientRect());
      const rail = [...stage.querySelectorAll(".seat, .seat-cards, .seat-outcome-badges")];
      const box = stage.getBoundingClientRect();
      return {
        onBoard: rail.filter((node) => middle.some((card) => overlaps(node.getBoundingClientRect(), card))).length,
        escaping: rail.filter((node) => {
          const rect = node.getBoundingClientRect();
          return rect.bottom > box.bottom + 1 || rect.top < box.top - 1;
        }).length,
      };
    });
    expect(geometry.onBoard, `D1: seats must clear the board at ${height}px`).toBe(0);
    expect(geometry.escaping, `D2: no seat may hang off the stage at ${height}px`).toBe(0);
    expect(
      await page.evaluate(() => document.documentElement.scrollHeight <= document.documentElement.clientHeight),
      `D3: the table must fit a ${height}px window`,
    ).toBe(true);
    const shellGeometry = await page.locator(".table-shell").evaluate((shell) => {
      const stage = shell.querySelector(".table-stage");
      const decision = shell.querySelector(".decision-area");
      const stageContent = [...stage.querySelectorAll(".seat, .seat-cards, .seat-outcome-badges, .table-center, .showdown-advance")]
        .map((node) => node.getBoundingClientRect());
      const contentBottom = Math.max(...stageContent.map((rect) => rect.bottom));
      const decisionTop = decision.getBoundingClientRect().top;
      return {
        contentBottom,
        decisionTop,
        overlapsDecision: contentBottom > decisionTop - 8,
      };
    });
    expect(shellGeometry.overlapsDecision, `V43: completed showdown content must clear actions at ${height}px ${JSON.stringify(shellGeometry)}`).toBe(false);
    /* oxlint-enable no-await-in-loop */
  }
});

test("keeps completed desktop showdowns inside their stage", async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 700 });
  await mountTable(page, showdownState);
  await expect(page.locator(".seat")).toHaveCount(6);
  await expect(page.locator(".showdown-result")).toContainText("Mina wins $400");
  const geometry = await page.locator(".table-shell").evaluate((shell) => {
    const stage = shell.querySelector(".table-stage");
    const decision = shell.querySelector(".decision-area");
    const center = [...stage.querySelectorAll(".board .playing-card, .table-metrics, .showdown-result, .showdown-advance")]
      .map((node) => node.getBoundingClientRect());
    const rail = [...stage.querySelectorAll(".seat, .seat-cards, .seat-outcome-badges")]
      .map((node) => node.getBoundingClientRect());
    // Browser-evaluated helpers cannot close over test-scope functions.
    // oxlint-disable-next-line unicorn/consistent-function-scoping
    const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    const contentBottom = Math.max(...[...center, ...rail].map((rect) => rect.bottom));
    const decisionTop = decision.getBoundingClientRect().top;
    return {
      railOverCenter: rail.filter((rect) => center.some((middle) => overlaps(rect, middle))).length,
      contentBottom,
      decisionTop,
      stageBottom: stage.getBoundingClientRect().bottom,
      overlapsDecision: contentBottom > decisionTop - 8,
    };
  });
  expect(geometry.railOverCenter, `V43: completed showdown rail must clear center content ${JSON.stringify(geometry)}`).toBe(0);
  expect(geometry.overlapsDecision, `V43: completed showdown content must clear actions ${JSON.stringify(geometry)}`).toBe(false);
});

test("keeps your own cards off the board at every size", async ({ page }) => {
  const headsUp = {
    ...tableState,
    viewer_seat: 2,
    seats: tableState.seats.map((seat, index) =>
      index === 1 || index === 2 ? seat : { ...seat, occupant: "empty", display_name: null }),
  };
  // Your hole cards hang up toward the board, and the size slider makes them
  // bigger, so both ends of the range have to clear it.
  for (const height of [900, 1000, 1400]) {
    /* oxlint-disable no-await-in-loop */
    for (const size of ["50", "100", "200"]) {
      await page.setViewportSize({ width: 1000, height });
      await page.goto("/card-test");
      await page.evaluate((value) => localStorage.setItem("table-card-size-percent", value), size);
      await mountTable(page, headsUp);
      await page.locator(".seat.viewer .seat-cards").waitFor();
      const geometry = await page.locator(".table-stage").evaluate((stage) => {
        // Browser-evaluated helpers cannot close over test-scope functions.
        // oxlint-disable-next-line unicorn/consistent-function-scoping
        const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
        // The middle of the table is what must stay clear: the board and the pot.
        const centre = [...stage.querySelectorAll(".board .playing-card, .table-metrics")]
          .map((node) => node.getBoundingClientRect());
        const box = stage.getBoundingClientRect();
        const cards = stage.querySelector(".seat.viewer .seat-cards").getBoundingClientRect();
        const wager = stage.querySelector(".seat.viewer .seat-wager:not(.no-wager)")?.getBoundingClientRect();
        return {
          onCentre: centre.some((node) => overlaps(cards, node)),
          escapes: cards.top < box.top - 1 || cards.bottom > box.bottom + 1,
          width: Math.round(cards.width),
          wagerOnCentre: wager ? centre.some((node) => overlaps(wager, node)) : false,
        };
      });
      expect(geometry.onCentre, `D4: your cards must clear the board at ${height}px, ${size}%`).toBe(false);
      expect(geometry.escapes, `D5: your cards must stay on the stage at ${height}px, ${size}%`).toBe(false);
      expect(geometry.width, `D6: your cards must stay legible at ${height}px, ${size}%`).toBeGreaterThan(40);
      expect(geometry.wagerOnCentre, `D7: your wager must clear the board at ${height}px, ${size}%`).toBe(false);
    }
    /* oxlint-enable no-await-in-loop */
  }
  await page.evaluate(() => localStorage.removeItem("table-card-size-percent"));
});

test("keeps the table log footprint stable as events accumulate", async ({ page }) => {
  await mountTable(page, tableState);
  const log = page.locator(".game-log");
  await log.locator("ol").evaluate((list) => list.replaceChildren(list.firstElementChild));
  const sparse = await log.evaluate((node) => {
    const logBox = node.getBoundingClientRect();
    const controls = document.querySelector(".table-controls").getBoundingClientRect();
    return { height: logBox.height, controlsTop: controls.top, controlsGap: controls.top - logBox.bottom };
  });
  await log.locator("ol").evaluate((list) => {
    const row = list.firstElementChild;
    for (let index = 0; index < 30; index += 1) list.append(row.cloneNode(true));
  });
  const dense = await log.evaluate((node) => {
    const logBox = node.getBoundingClientRect();
    const controls = document.querySelector(".table-controls").getBoundingClientRect();
    return {
      height: logBox.height,
      controlsTop: controls.top,
      controlsGap: controls.top - logBox.bottom,
      containsEvents: node.scrollHeight >= node.clientHeight,
    };
  });
  expect(Math.abs(dense.height - sparse.height), "V22: table log height must not meaningfully grow with events").toBeLessThan(1);
  expect(dense.controlsTop, "V22: content below table log must remain fixed").toBe(sparse.controlsTop);
  expect(sparse.controlsGap, "V22: extra vertical space should belong to the table log, not an empty row above controls").toBeLessThanOrEqual(16);
  expect(dense.controlsGap, "V22: extra vertical space should belong to the table log, not an empty row above controls").toBeLessThanOrEqual(16);
  expect(dense.containsEvents, "V22: table events must stay inside the fixed log region").toBe(true);
});

test("makes leaving a tournament a deliberate forfeit", async ({ page }) => {
  let left = 0;
  await page.route("**/tables/mock/leave", async (route) => {
    left += 1;
    await route.fulfill({ json: { ok: true } });
  });

  // A cash table pays you out, so leaving stays one click.
  await mountTable(page, tableState);
  await page.locator(".table-controls .table-command").click();
  await expect.poll(() => left).toBe(1);
  await expect(page.locator(".confirm-dialog")).toHaveCount(0);

  const tournament = {
    ...tableState,
    buy_in: 50_000,
    tournament: { level: 1, small_blind: 100, big_blind: 200, ante: 0, hands_at_level: 3, hands_per_level: 12, started: true, finished: false, registered: 6, seat_count: 6 },
  };
  await mountTable(page, tournament);
  await page.locator(".table-controls .table-command").click();
  const confirm = page.locator(".confirm-dialog");
  await expect(confirm).toBeVisible();
  await expect(confirm).toContainText("You forfeit your entry");
  await expect(confirm).toContainText("$500 buy-in stays in the prize pool");

  // Backing out leaves the seat alone.
  await page.getByRole("button", { name: "Keep playing" }).click();
  await expect(confirm).not.toBeVisible();
  expect(left, "declining must not leave the table").toBe(1);

  await page.locator(".table-controls .table-command").click();
  await page.getByRole("button", { name: "Forfeit and leave" }).click();
  await expect(confirm).not.toBeVisible();
  await expect.poll(() => left).toBe(2);

  const eliminated = {
    ...tournament,
    hand: null,
    viewer_seat: null,
    viewer_eliminated: true,
    tournament: { ...tournament.tournament, finish_order: [tableState.viewer_seat] },
    seats: tournament.seats.map((seat) => {
      if (seat.index !== tableState.viewer_seat) return seat;
      return Object.assign({}, seat, { stack: 0 });
    }),
  };
  await mountTable(page, eliminated);
  await page.locator(".table-controls .table-command").click();
  await expect.poll(() => left).toBe(3);
  await expect(page.locator(".confirm-dialog")).toHaveCount(0);
});

test("offers a seat at a table the house has filled", async ({ page }) => {
  // Every seat taken by the house, nobody sitting down.
  const houseTable = {
    ...tableState,
    viewer_seat: null,
    hand: null,
    can_deal: true,
    seats: tableState.seats.map((seat, index) => Object.assign({}, seat, {
      occupant: `Bot ${index}`,
      bot: true,
      display_name: `Bot ${index}`,
    })),
  };
  await mountTable(page, houseTable);
  // A full table of house players is still a table you can join.
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Buy In $200"]);
  await expect(page.locator(".table-controls .table-command")).toBeEnabled();
  // Short of the buy-in a lending table covers the difference, so the seat is
  // still offered; a table that lends nothing refuses it.
  await mountTable(page, { ...houseTable, bank_balance: 19_999 });
  await expect(page.locator(".table-controls .table-command")).toBeEnabled();
  await mountTable(page, { ...houseTable, bank_balance: 19_999, lends_buy_in: false });
  await expect(page.locator(".table-controls .table-command")).toBeDisabled();
  await mountTable(page, houseTable);
  // And it waits to be asked before it plays.
  await expect(page.getByRole("button", { name: "Deal a hand" })).toBeVisible();

  // After a hand, no next one is coming on its own, so once the result has had
  // its moment the table offers to deal again rather than counting down forever.
  const finished = {
    ...houseTable,
    last_hand: showdownState.last_hand,
    result_pause_seconds: 6,
    next_hand_at: new Date(Date.now() - 1_000).toISOString(),
  };
  await mountTable(page, finished);
  await expect(page.locator(".showdown-result")).toContainText("wins $400");
  await expect(page.getByRole("button", { name: "Deal a hand" })).toBeVisible();
  await expect(page.locator(".showdown-advance.spectator")).toHaveCount(0);

  // While the result is still counting down it keeps the countdown.
  const settling = {
    ...finished,
    next_hand_at: new Date(Date.now() + 5_000).toISOString(),
  };
  await mountTable(page, settling);
  await expect(page.getByRole("button", { name: "Deal a hand" })).toHaveCount(0);

  // A table full of people has no room, and offers nothing.
  const packed = {
    ...houseTable,
    can_deal: false,
    seats: houseTable.seats.map((seat) => Object.assign({}, seat, { occupant: "human", bot: false })),
  };
  await mountTable(page, packed);
  await expect(page.locator(".table-controls .table-command")).toHaveCount(0);

  // The bar and the log keep their place whether or not they have anything in
  // them, so the table above does not jump about.
  const bands = async () => {
    await page.locator(".table-stage").waitFor();
    return page.evaluate(() => ({
      decision: document.querySelectorAll(".decision-area").length,
      log: document.querySelectorAll(".game-log").length,
      stage: Math.round(document.querySelector(".table-stage").getBoundingClientRect().height),
      logHeight: Math.round(document.querySelector(".game-log").getBoundingClientRect().height),
      controlsTop: Math.round(document.querySelector(".table-controls").getBoundingClientRect().top),
    }));
  };
  await mountTable(page, houseTable);
  const idle = await bands();
  await mountTable(page, tableState);
  const playing = await bands();
  expect(idle.decision, "an idle table reserves the action bar").toBe(1);
  expect(idle.log, "and the log").toBe(1);
  expect(playing.decision).toBe(1);
  expect(playing.log).toBe(1);
  expect(Math.abs(playing.controlsTop - idle.controlsTop), "controls stay anchored while stage and log exchange spare height").toBeLessThanOrEqual(2);
  if ((page.viewportSize()?.width || 0) <= 640) {
    expect(idle.logHeight, "V48: an idle table gives its spare vertical space to the log").toBeGreaterThan(playing.logHeight);
  } else {
    expect(Math.abs(playing.stage - idle.stage), "desktop table keeps its fixed stage height").toBeLessThan(20);
  }
});

test("keeps the desktop table shell dense", async ({ page }) => {
  for (const viewport of [{ width: 1280, height: 832 }, { width: 1680, height: 1050 }]) {
    /* oxlint-disable no-await-in-loop */
    await page.setViewportSize(viewport);
    await mountTable(page, tableState);
    const geometry = await page.locator(".table-shell").evaluate((shell) => {
      const viewer = shell.querySelector(".seat.viewer").getBoundingClientRect();
      const decision = shell.querySelector(".decision-area").getBoundingClientRect();
      const buttons = [...shell.querySelectorAll(".actions button")].map((button) => button.getBoundingClientRect());
      return {
        viewerDecisionGap: decision.top - viewer.bottom,
        actionBandExcess: decision.height - Math.max(...buttons.map((button) => button.height)),
      };
    });
    expect(geometry.viewerDecisionGap, `desktop viewer seat must stay near the decision area at ${JSON.stringify(viewport)} ${JSON.stringify(geometry)}`).toBeLessThan(viewport.height * 0.1);
    expect(geometry.actionBandExcess, `desktop action band must hug its controls at ${JSON.stringify(viewport)} ${JSON.stringify(geometry)}`).toBeLessThanOrEqual(24);
    const seatGeometry = await page.locator(".table-stage").evaluate((stage) => {
      return [...stage.querySelectorAll(".seat")].every((seat) => {
        const seatBox = seat.getBoundingClientRect();
        const stack = seat.querySelector(".seat-stack")?.getBoundingClientRect();
        return [...seat.querySelectorAll(".playing-card")].every((card) => {
          const cardBox = card.getBoundingClientRect();
          return cardBox.top >= seatBox.top && cardBox.bottom <= seatBox.bottom
            && cardBox.left >= seatBox.left && cardBox.right <= seatBox.right
            && (!stack || !(cardBox.left < stack.right && cardBox.right > stack.left && cardBox.top < stack.bottom && cardBox.bottom > stack.top));
        });
      });
    });
    expect(seatGeometry, `desktop seat cards must stay inside seats and clear stacks at ${JSON.stringify(viewport)}`).toBe(true);
    const commandControls = await page.locator(".table-controls :is(.table-history-link,.table-command-link,.table-command,.seat-bot button)").evaluateAll((controls) =>
      controls.map((control) => ({ fontSize: getComputedStyle(control).fontSize, height: control.getBoundingClientRect().height })),
    );
    expect(new Set(commandControls.map((control) => control.fontSize)).size, `desktop table command fonts must match at ${JSON.stringify(viewport)}`).toBe(1);
    expect(new Set(commandControls.map((control) => control.height)).size, `desktop table command heights must match at ${JSON.stringify(viewport)}`).toBe(1);
    await mountTable(page, showdownState);
    await expect(page.locator(".showdown-result")).toContainText("Mina wins $400");
    const showdownGeometry = await page.locator(".table-shell").evaluate((shell) => {
      const board = shell.querySelector(".table-center > .board").getBoundingClientRect();
      const rail = shell.querySelector(".game-log").getBoundingClientRect();
      const result = shell.querySelector(".showdown-result");
      return { boardRight: board.right, railLeft: rail.left, resultClipped: result.scrollWidth > result.clientWidth };
    });
    expect(showdownGeometry.boardRight, `desktop showdown board must clear the game log rail at ${JSON.stringify(viewport)} ${JSON.stringify(showdownGeometry)}`).toBeLessThanOrEqual(showdownGeometry.railLeft);
    expect(showdownGeometry.resultClipped, `desktop showdown result must remain readable at ${JSON.stringify(viewport)} ${JSON.stringify(showdownGeometry)}`).toBe(false);
    /* oxlint-enable no-await-in-loop */
  }
});

test("offers one state-aware table lifecycle command", async ({ page }) => {
  await mountTable(page, tableState);
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Leave"]);
  await expect(page.getByRole("button", { name: "Sit out" })).toHaveCount(0);
  const expectControlsTappable = async () => {
    const controlBounds = await page.locator(".table-controls :is(.table-history-link,.table-command)").evaluateAll((controls) => {
      const safeBottom = Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--safe-bottom"));
      return controls.map((control) => {
        const rect = control.getBoundingClientRect();
        const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
        const range = document.createRange();
        range.selectNodeContents(control);
        const label = range.getBoundingClientRect();
        return {
          label: control.textContent?.trim(),
          left: rect.left,
          right: rect.right,
          bottom: rect.bottom,
          height: rect.height,
          labelInside: label.left >= rect.left - 1 && label.right <= rect.right + 1 && label.top >= rect.top - 1 && label.bottom <= rect.bottom + 1,
          overflow: getComputedStyle(control).overflow,
          safeBottom,
          tappable: hit === control || control.contains(hit),
          viewportHeight: window.innerHeight,
          viewportWidth: window.innerWidth,
        };
      });
    });
    for (const bounds of controlBounds) {
      expect(bounds.left, `V54: ${bounds.label} must remain inside the left edge ${JSON.stringify(bounds)}`).toBeGreaterThanOrEqual(0);
      expect(bounds.right, `V54: ${bounds.label} must remain inside the right edge ${JSON.stringify(bounds)}`).toBeLessThanOrEqual(bounds.viewportWidth);
      expect(bounds.bottom, `V54: ${bounds.label} must clear the PWA home indicator ${JSON.stringify(bounds)}`).toBeLessThanOrEqual(bounds.viewportHeight - bounds.safeBottom + 1);
      expect(bounds.height, `V54: ${bounds.label} retains a usable mobile tap target`).toBeGreaterThanOrEqual(40);
      expect(bounds.tappable, `V54: ${bounds.label} must be the topmost target at its center ${JSON.stringify(bounds)}`).toBe(true);
      expect(bounds.labelInside || bounds.overflow === "hidden", `V54: ${bounds.label} label must not paint outside its control ${JSON.stringify(bounds)}`).toBe(true);
    }
  };
  await expectControlsTappable();

  let joinBody;
  await page.route("**/tables/mock/join", async (route) => {
    joinBody = route.request().postDataJSON();
    await route.fulfill({ json: { ok: true } });
  });
  const unseated = {
    ...tableState,
    viewer_seat: null,
    hand: null,
    seats: tableState.seats.map((seat) => seat.index === 2
      ? { ...seat, occupant: "empty", display_name: null, stack: 0 }
      : seat),
  };
  await mountTable(page, unseated);
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Buy In $200"]);
  await expect(page.locator(".table-controls .table-command")).toBeEnabled();
  await expectControlsTappable();
  await mountTable(page, { ...unseated, bank_balance: 2_500_000, buy_in: 2_000_000 });
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Buy In $20,000"]);
  await expectControlsTappable();
  // A long buy-in label takes the room its neighbours leave, and if the row is
  // genuinely too narrow the ellipsis comes with a native tooltip.
  const buyInLabel = await page.locator(".table-controls .table-command").evaluate((control) => ({
    clipped: control.scrollWidth > control.clientWidth + 1,
    title: control.getAttribute("title"),
    wide: window.innerWidth >= 1024,
  }));
  expect(buyInLabel.title, `V54: a clipped command names itself in a tooltip ${JSON.stringify(buyInLabel)}`)
    .toBe(buyInLabel.clipped ? "Buy In $20,000" : null);
  if (buyInLabel.wide) {
    expect(buyInLabel.clipped, `V54: the side rail has room for the whole buy-in ${JSON.stringify(buyInLabel)}`).toBe(false);
  }
  // Seating a bot is one click: pick the type and the server fills the next seat.
  let botBody;
  await page.route("**/tables/mock/bot", async (route) => {
    botBody = route.request().postDataJSON();
    await route.fulfill({ json: { ok: true } });
  });
  // V62: the stakes decide who the house will sit. A $20,000 seat is past the
  // shark-only rung, so a shark is the only thing on offer here.
  await expect(page.locator(".seat-bot button")).toHaveText(["Seat shark"]);
  await page.getByRole("button", { name: "Seat shark" }).click();
  expect(botBody).toEqual({ kind: "shark" });
  // A $1,000 seat has dropped the fish but keeps the rest; the cheapest keeps
  // everyone.
  await mountTable(page, { ...unseated, bank_balance: 2_500_000, buy_in: 100_000 });
  await expect(page.locator(".seat-bot button")).toHaveText(["Seat rock", "Seat grinder", "Seat shark"]);
  await mountTable(page, { ...unseated, bank_balance: 2_500_000 });
  await expect(page.locator(".seat-bot button")).toHaveText(["Seat fish", "Seat rock", "Seat grinder", "Seat shark"]);
  await page.getByRole("button", { name: "Seat rock" }).click();
  expect(botBody).toEqual({ kind: "rock" });
  await page.locator(".table-controls .table-command").click();
  expect(joinBody).toEqual({});

  let rebuyBody;
  await page.route("**/tables/mock/rebuy", async (route) => {
    rebuyBody = route.request().postDataJSON();
    await route.fulfill({ json: { ok: true } });
  });
  const busted = {
    ...tableState,
    hand: null,
    seats: tableState.seats.map((seat) => seat.index === tableState.viewer_seat
      ? { ...seat, stack: 0 }
      : seat),
  };
  await mountTable(page, busted);
  // Busted, you may buy in again -- or walk away. Rebuying must never be the
  // only way out of a seat.
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Re-Buy In $200", "Leave"]);
  await expect(page.locator(".table-controls .table-command").first()).toBeEnabled();
  // The rebuy borrows what the balance is short, unless this table lends none.
  await mountTable(page, { ...busted, bank_balance: 19_999 });
  await expect(page.locator(".table-controls .table-command").first()).toBeEnabled();
  await mountTable(page, { ...busted, bank_balance: 19_999, lends_buy_in: false });
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Re-Buy In $200", "Leave"]);
  await expect(page.locator(".table-controls .table-command").first()).toBeDisabled();
  await mountTable(page, busted);
  await page.locator(".table-controls .table-command").first().click();
  expect(rebuyBody).toEqual({});

  // A cash table whose only person is busted is a table of house players
  // again: it stops dealing on its own and offers the deal button.
  const bustedBetweenHands = {
    ...tableState,
    hand: null,
    can_deal: true,
    seats: tableState.seats.map((seat) => (seat.index === tableState.viewer_seat
      ? Object.assign({}, seat, { stack: 0 })
      : seat)),
  };
  await mountTable(page, bustedBetweenHands);
  await expect(page.getByRole("button", { name: "Deal a hand" })).toBeVisible();

  // Busted mid-hand you cannot rebuy yet, but leaving is still on offer.
  const bustedMidHand = {
    ...tableState,
    seats: tableState.seats.map((seat) => (seat.index === tableState.viewer_seat
      ? Object.assign({}, seat, { stack: 0 })
      : seat)),
  };
  await mountTable(page, bustedMidHand);
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Leave"]);

  await mountTable(page, { ...tableState, viewer_leaving: true });
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Leaving..."]);
  await expect(page.locator(".table-controls .table-command")).toBeDisabled();
});

test("celebrates a completed tournament and leaves to the lobby", async ({ page }) => {
  const finished = {
    ...showdownState,
    tournament: { level: 4, small_blind: 800, big_blind: 1_600, ante: 0, hands_at_level: 12, hands_per_level: 12, started: true, finished: true, registered: 6, seat_count: 6, finish_order: [2, 3, 4, 5, 0] },
    result_pause_seconds: 0,
    next_hand_at: new Date(Date.now() - 1_000).toISOString(),
    seats: showdownState.seats.map((seat) => {
      if (seat.index === 1) return Object.assign({}, seat, { stack: 120_000 });
      if (seat.index === 0) return Object.assign({}, seat, { stack: 0 });
      return seat;
    }),
  };
  await mountTable(page, finished);
  await expect(page.locator(".tournament-complete")).toContainText("Tournament complete");
  await expect(page.locator(".tournament-complete")).toContainText("Mina wins");
  await expect(page.locator(".seat.champion")).toContainText("Mina");
  await expect(page.locator(".seat.champion")).toHaveCSS("border-top-color", "rgb(241, 213, 110)");
  await expect(page.locator(".showdown-advance")).toHaveCount(0);
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Leave"]);
  await expect(page.locator(".table-controls .table-command")).toHaveAttribute("href", "/tables");
  await expect(page.locator(".confirm-dialog")).toHaveCount(0);
});

test("reflows viewer cards at maximum display settings", async ({ page }) => {
  await mountTable(page, { ...tableState, hand: { ...tableState.hand, your_hole_cards: ["Tc", "9c"] } });
  await page.getByRole("button", { name: "Card display settings" }).click();
  await page.locator('input[name="card-scale"]').fill("200");
  await page.locator('input[name="rank-scale"]').fill("200");
  await page.locator('input[name="rank-weight"]').fill("200");
  await expect.poll(() => page.evaluate(() => document.documentElement.style.getPropertyValue("--viewer-card-w"))).toBe("10.8rem");
  await page.getByRole("button", { name: "Close" }).click();
  const geometry = await page.locator(".table-stage").evaluate((stage) => {
    const viewerSeat = stage.querySelector(".seat.viewer").getBoundingClientRect();
    const viewerCards = stage.querySelector(".seat.viewer .seat-cards").getBoundingClientRect();
    const viewerWager = stage.querySelector(".seat.viewer .seat-wager")?.getBoundingClientRect();
    const tableCenter = stage.querySelector(".table-center").getBoundingClientRect();
    const card = stage.querySelector(".seat.viewer .playing-card");
    const corners = [...card.querySelectorAll(".card-corner b, .card-corner i")].map((node) => node.getBoundingClientRect());
    const cardBox = card.getBoundingClientRect();
    // Browser-evaluated helpers cannot close over test-scope functions.
    // oxlint-disable-next-line unicorn/consistent-function-scoping
    const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    return {
      centerOverlap: overlaps(viewerCards, tableCenter),
      wagerOverlap: viewerWager ? overlaps(viewerWager, tableCenter) : false,
      cardsEscapeSeat: viewerCards.top < viewerSeat.top || viewerCards.bottom > viewerSeat.bottom || viewerCards.left < viewerSeat.left || viewerCards.right > viewerSeat.right,
      faceOverlap: corners.some((corner, index) => corners.slice(index + 1).some((other) => overlaps(corner, other))),
      faceEscapesCard: corners.some((corner) => corner.top < cardBox.top || corner.bottom > cardBox.bottom || corner.left < cardBox.left || corner.right > cardBox.right),
      viewerCards: { top: viewerCards.top, bottom: viewerCards.bottom },
      viewerSeat: { top: viewerSeat.top, bottom: viewerSeat.bottom },
      tableCenter: { top: tableCenter.top, bottom: tableCenter.bottom },
      corners: corners.map(({ top, right, bottom, left }) => ({ top, right, bottom, left })),
    };
  });
  expect(geometry.centerOverlap, `V21: max-size viewer cards must clear table center content ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.wagerOverlap, `V21: max-size viewer wager must clear table center content ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.cardsEscapeSeat, `V21: max-size viewer cards must stay inside the viewer seat ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.faceOverlap, `V21: max-size rank and suit must not collide ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.faceEscapesCard, `V21: max-size rank and suit must stay on the card ${JSON.stringify(geometry)}`).toBe(false);
  await expectLayout(page, "max-card-table", TABLE_LAYOUT);
  await expectImage(page, "max-card-table.png", { fullPage: true });
});

test("integrates showdown with players and table log", async ({ page }) => {
  let continued = false;
  await page.route("**/tables/mock/continue", (route) => {
    continued = true;
    return route.fulfill({ json: { ok: true } });
  });
  await mountTable(page, showdownState);
  await expect(page.locator(".seat.winner")).toHaveCount(1);
  await expect(page.locator(".seat .seat-cards.revealed")).toHaveCount(2);
  await expect(page.locator(".seat:not(.viewer) .seat-cards .card-zoom-target")).toHaveCount(0);
  await expect(page.locator(".showdown-result")).toContainText("Mina wins $400");
  await expect(page.locator(".game-log")).toContainText("Mina wins $400");
  const opponentCard = page.locator(".seat:not(.viewer) .seat-cards.revealed .playing-card").first();
  expect((await opponentCard.boundingBox()).width).toBeGreaterThan(28);
  await expect(page.locator(".seat.winner .winner-role")).toHaveText("WINNER");
  await expect(page.locator(".seat.winner")).toHaveCSS("border-top-color", "rgb(241, 213, 110)");
  const winnerBadge = await page.locator(".seat.winner").evaluate((seat) => {
    const seatBox = seat.getBoundingClientRect();
    const badgeBox = seat.querySelector(".winner-role").getBoundingClientRect();
    const stageBox = document.querySelector(".table-stage").getBoundingClientRect();
    return {
      attachedBelow: badgeBox.top >= seatBox.bottom - 1,
      clipped: badgeBox.bottom > stageBox.bottom + 1,
    };
  });
  if ((page.viewportSize()?.width || 0) <= 640 || (page.viewportSize()?.height || 0) <= 520) {
    expect(winnerBadge.attachedBelow, "S8: the compact winner badge should stay inside the seat").toBe(false);
  } else {
    expect(winnerBadge.attachedBelow, "S8: the winner badge should hang below the seat").toBe(true);
  }
  expect(winnerBadge.clipped, "S9: the winner badge must not be clipped").toBe(false);
  await expect(page.locator(".showdown-advance button")).toContainText("OK · 6s");
  await expect(page.locator(".showdown-progress")).toHaveCSS("width", /.+/);
  await expect(page.locator(".last-hand")).toHaveCount(0);
  await expectLayout(page, "showdown-table", TABLE_LAYOUT);
  await expectImage(page, "showdown-table.png", { fullPage: true });
  await page.locator(".showdown-advance button").click();
  expect(continued).toBe(true);
});

test("emulates the target phone's safe-area insets", async ({ page }) => {
  // Chromium reports every inset as 0, so a silent break here would quietly
  // return mobile snapshots to a notchless phone that nobody owns.
  test.skip((page.viewportSize()?.width || 0) > 640, "V54: only the phone project pins insets");
  await page.goto("/card-test");
  const pwaSurface = await page.evaluate(() => {
    const styles = getComputedStyle(document.documentElement);
    return {
      background: styles.backgroundColor,
      insets: ["top", "right", "bottom", "left"].map((side) => styles.getPropertyValue(`--safe-${side}`).trim()),
    };
  });
  expect(pwaSurface.insets, "V54: the full-bleed PWA takes the notch on top and the home indicator below").toEqual([
    "59px",
    "0px",
    "34px",
    "0px",
  ]);
  expect(pwaSurface.background, "V54: the status-bar canvas must carry the felt instead of defaulting to black").not.toBe("rgba(0, 0, 0, 0)");

  await page.locator("style[data-device]").evaluate((style) => {
    style.textContent = ":root{--safe-top:0px;--safe-right:0px;--safe-bottom:0px;--safe-left:0px}";
  });
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--safe-bottom").trim()), "V54: standalone iPhones must retain home-indicator clearance when WebKit reports a zero inset").toBe("34px");
});

test("packs the table into a landscape phone without scrolling", async ({ page }) => {
  await useDevice(page, IPHONE_LANDSCAPE);
  await mountTable(page, tableState);
  await expect(page.locator(".seat.viewer")).toBeVisible();
  const rail = await page.locator(".table-stage").evaluate((stage) => {
    const seats = [...stage.querySelectorAll(".seat")];
    const viewer = stage.querySelector(".seat.viewer").getBoundingClientRect();
    const viewerCards = stage.querySelector(".seat.viewer .seat-cards").getBoundingClientRect();
    const others = seats.filter((seat) => !seat.classList.contains("viewer")).map((seat) => seat.getBoundingClientRect());
    const board = [...stage.querySelectorAll(".board .playing-card, .table-metrics")].map((node) => node.getBoundingClientRect());
    // Browser-evaluated helpers cannot close over test-scope functions.
    // oxlint-disable-next-line unicorn/consistent-function-scoping
    const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    return {
      otherRows: new Set(others.map((seat) => Math.round(seat.top))).size,
      viewerWidth: viewer.width,
      otherWidth: Math.max(...others.map((rect) => rect.width)),
      boardOverlap: seats.some((seat) => board.some((card) => overlaps(seat.getBoundingClientRect(), card))),
      viewerCardsClipped: viewerCards.top < viewer.top - 1 || viewerCards.bottom > viewer.bottom + 1 || viewerCards.left < viewer.left - 1 || viewerCards.right > viewer.right + 1,
      viewer: { top: viewer.top, right: viewer.right, bottom: viewer.bottom, left: viewer.left },
      viewerCards: { top: viewerCards.top, right: viewerCards.right, bottom: viewerCards.bottom, left: viewerCards.left },
      stageScrolls: stage.scrollHeight > stage.clientHeight,
    };
  });
  expect(rail.otherRows, "L1: landscape opponents must stay on one row").toBe(1);
  expect(rail.viewerWidth, "L2: the viewer seat must take its own row").toBeGreaterThan(rail.otherWidth * 1.8);
  expect(rail.boardOverlap, "L3: the rail must not collide with the board").toBe(false);
  expect(rail.viewerCardsClipped, `L5: landscape viewer cards must not be clipped ${JSON.stringify(rail)}`).toBe(false);
  expect(rail.stageScrolls, "L4: a landscape table must fit its stage").toBe(false);
  expect(await page.evaluate(() => document.documentElement.scrollHeight <= document.documentElement.clientHeight)).toBe(true);
  await expectLayout(page, "landscape-table", TABLE_LAYOUT);
  await expectImage(page, "landscape-table.png", { fullPage: true });
});

test("packs the table into a portrait phone without scrolling", async ({ page }) => {
  for (const device of [IPHONE_PORTRAIT, IPHONE_SE_PORTRAIT, IPHONE_MAX_PORTRAIT]) {
    /* oxlint-disable no-await-in-loop */
    const viewport = device.viewport;
    await useDevice(page, device);
    await page.goto("/card-test");
    await page.evaluate(() => localStorage.setItem("table-card-size-percent", "200"));
    await mountTable(page, tableState);
    await expect(page.locator(".seat.viewer")).toBeVisible();

    const layout = await page.locator(".table-shell").evaluate((shell) => {
      const actionButtons = [...shell.querySelectorAll(".actions button")].map((button) => {
        const rect = button.getBoundingClientRect();
        const styles = getComputedStyle(button);
        return {
          top: Math.round(rect.top),
          height: Math.round(rect.height),
          fontSize: styles.fontSize,
          scrollWidth: button.scrollWidth,
          clientWidth: button.clientWidth,
          scrollHeight: button.scrollHeight,
          clientHeight: button.clientHeight,
        };
      });
      const stage = shell.querySelector(".table-stage");
      const stageBox = stage.getBoundingClientRect();
      const viewer = shell.querySelector(".seat.viewer").getBoundingClientRect();
      const viewerSummary = [...shell.querySelectorAll(".seat.viewer .viewer-summary > :not(.player-tooltip)")]
        .map((node) => node.getBoundingClientRect());
      const board = [...shell.querySelectorAll(".board .playing-card, .table-metrics")].map((node) => node.getBoundingClientRect());
      const metrics = shell.querySelector(".table-metrics").getBoundingClientRect();
      const metricRows = [...shell.querySelectorAll(".table-metrics > span")].map((node) => node.getBoundingClientRect());
      const sharedCards = shell.querySelector(".table-center > .board").getBoundingClientRect();
      const controls = shell.querySelector(".table-controls").getBoundingClientRect();
      const decision = shell.querySelector(".decision-area").getBoundingClientRect();
      const pageNode = shell.closest(".page");
      // Browser-evaluated helpers cannot close over test-scope functions.
      // oxlint-disable-next-line unicorn/consistent-function-scoping
      const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
      return {
        documentScrolls: document.documentElement.scrollHeight > document.documentElement.clientHeight,
        stageScrolls: stage.scrollHeight > stage.clientHeight,
        viewerEscapesStage: viewer.top < stageBox.top - 1 || viewer.bottom > stageBox.bottom + 1,
        viewerOverlapsBoard: board.some((node) => overlaps(viewer, node)),
        viewerSummaryLeftSpread: Math.max(...viewerSummary.map((node) => node.left))
          - Math.min(...viewerSummary.map((node) => node.left)),
        viewerSummaryStacked: viewerSummary.every((node, index) => index === 0 || node.top >= viewerSummary[index - 1].bottom - 1),
        metricsStacked: metricRows.length < 2 || metricRows[1].top >= metricRows[0].bottom,
        metricsLeftOfBoard: metrics.right <= sharedCards.left,
        controlsBottomGap: document.documentElement.clientHeight - controls.bottom,
        safeBottom: Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--safe-bottom")),
        actionBandExcess: decision.height - Math.max(...actionButtons.map((button) => button.height)),
        pageBottomPadding: Number.parseFloat(getComputedStyle(pageNode).paddingBottom),
        actionButtons,
      };
    });
    expect(layout.documentScrolls, `V42: portrait poker must not scroll the document at ${JSON.stringify(viewport)}`).toBe(false);
    expect(layout.stageScrolls, `V42: portrait poker must not scroll inside the table stage at ${JSON.stringify(viewport)}`).toBe(false);
    expect(layout.viewerEscapesStage, `V42: portrait viewer seat must stay inside the stage at ${JSON.stringify(viewport)}`).toBe(false);
    expect(layout.viewerOverlapsBoard, `V42: portrait viewer seat must not collide with center cards at ${JSON.stringify(viewport)}`).toBe(false);
    expect(layout.viewerSummaryLeftSpread, `V48: viewer name, stack, and wager must share one left edge at ${JSON.stringify(viewport)}`).toBeLessThanOrEqual(1);
    expect(layout.viewerSummaryStacked, `V48: viewer name, stack, and wager must read down one column at ${JSON.stringify(viewport)}`).toBe(true);
    expect(layout.metricsStacked, `V48: pot and current bet must stack at ${JSON.stringify(viewport)}`).toBe(true);
    expect(layout.metricsLeftOfBoard, `V48: metrics must sit left of shared cards at ${JSON.stringify(viewport)}`).toBe(true);
    expect(layout.controlsBottomGap - layout.safeBottom, `V54: table controls must clear the home indicator at ${JSON.stringify(viewport)}`).toBeLessThanOrEqual(4);
    expect(layout.actionBandExcess, `V50: action background must hug controls at ${JSON.stringify(viewport)}`).toBeLessThanOrEqual(16);
    expect(layout.pageBottomPadding - layout.safeBottom, `V54: footer may reserve only the compact edge beyond the home indicator at ${JSON.stringify(viewport)}`).toBeLessThanOrEqual(4);
    expect(new Set(layout.actionButtons.map((button) => button.height)).size, "V42: portrait action buttons must share one height").toBe(1);
    expect(new Set(layout.actionButtons.map((button) => button.fontSize)).size, "V42: portrait action buttons must share one font size").toBe(1);
    expect(
      layout.actionButtons.every((button) => button.scrollWidth <= button.clientWidth && button.scrollHeight <= button.clientHeight),
      "V42: portrait action labels must fit their buttons",
    ).toBe(true);
    const seatGeometry = await page.locator(".table-stage").evaluate((stage) => {
      return [...stage.querySelectorAll(".seat")].flatMap((seat) => {
        const seatBox = seat.getBoundingClientRect();
        const stack = seat.querySelector(".seat-stack")?.getBoundingClientRect();
        return [...seat.querySelectorAll(".playing-card")].map((card) => {
          const cardBox = card.getBoundingClientRect();
          const ok = cardBox.top >= seatBox.top && cardBox.bottom <= seatBox.bottom
            && cardBox.left >= seatBox.left && cardBox.right <= seatBox.right
            && (!stack || !(cardBox.left < stack.right && cardBox.right > stack.left && cardBox.top < stack.bottom && cardBox.bottom > stack.top));
          return { name: seat.querySelector(".player-info")?.textContent?.trim(), ok, seat: [seatBox.left, seatBox.top, seatBox.right, seatBox.bottom], card: [cardBox.left, cardBox.top, cardBox.right, cardBox.bottom], stack: stack ? [stack.left, stack.top, stack.right, stack.bottom] : null };
        });
      });
    });
    expect(seatGeometry.every((card) => card.ok), `V42: portrait seat cards must stay inside seats and clear stacks at ${JSON.stringify(viewport)} ${JSON.stringify(seatGeometry)}`).toBe(true);
    const commandControls = await page.locator(".table-controls :is(.table-history-link,.table-command-link,.table-command,.seat-bot button)").evaluateAll((controls) =>
      controls.map((control) => ({ fontSize: getComputedStyle(control).fontSize, height: control.getBoundingClientRect().height })),
    );
    expect(new Set(commandControls.map((control) => control.fontSize)).size, `V42: portrait table command fonts must match at ${JSON.stringify(viewport)}`).toBe(1);
    expect(new Set(commandControls.map((control) => control.height)).size, `V42: portrait table command heights must match at ${JSON.stringify(viewport)}`).toBe(1);
    await mountTable(page, showdownState);
    const showdownLayout = await page.locator(".table-stage").evaluate((stage) => {
      const board = stage.querySelector(".table-center > .board").getBoundingClientRect();
      const rail = stage.querySelector(".table-center > .table-rail").getBoundingClientRect();
      const result = stage.querySelector(".showdown-result");
      return {
        boardRight: board.right,
        railLeft: rail.left,
        resultClipped: result.scrollWidth > result.clientWidth,
        resultText: result.textContent,
      };
    });
    expect(showdownLayout.boardRight, `V42: five-card showdown board must clear the result rail at ${JSON.stringify(viewport)}`).toBeLessThanOrEqual(showdownLayout.railLeft);
    expect(showdownLayout.resultClipped, `V42: five-card showdown result must not clip at ${JSON.stringify(viewport)}`).toBe(false);
    expect(showdownLayout.resultText, `V42: five-card showdown result must remain rendered at ${JSON.stringify(viewport)}`).toContain("Mina wins $400");
    /* oxlint-enable no-await-in-loop */
  }
  await page.evaluate(() => localStorage.removeItem("table-card-size-percent"));
});

// The board runs out on the server now, a street per advance, so the reveal is
// a sequence of states rather than a clock the client winds (SPEC V59).
const runoutSeats = structuredClone(showdownState.seats);
runoutSeats[1].stack = 22_400;

function allInRunout({ board, leaders, odds, awaiting = true }) {
  return {
    ...showdownState,
    hand: {
      ...tableState.hand,
      street: ["Preflop", "Preflop", "Preflop", "Flop", "Turn", "River"][board.length],
      board,
      current_player: null,
      legal_actions: null,
      to_call: 0,
      last_bet: 0,
      pot: 40_000,
      summary: null,
      awaiting_advance: awaiting,
      runout_leaders: leaders,
      runout_odds: odds,
      // Betting is closed, so both hands are face up for the rest of the hand.
      seats: [
        { index: 0, hole_cards: ["Kh", "Qh"] },
        { index: 1, hole_cards: ["Ac", "Ad"] },
      ],
      players: [
        { seat: 0, contribution: 20_000, street_contribution: 0, folded: false, all_in: true, acted: true, stack: 0 },
        { seat: 1, contribution: 20_000, street_contribution: 0, folded: false, all_in: true, acted: true, stack: 0 },
      ],
      events: [{ street: "Preflop", seat: 0, kind: "AllIn", amount: 20_000 }],
    },
    last_hand: null,
    next_hand_at: null,
    advance_at: new Date(Date.now() + 5_000).toISOString(),
    seats: runoutSeats,
  };
}

// A factory, not a value: `advance_at` is five seconds from whenever it is
// called, so a fixture built at file-load time would already be expired by the
// time a test that takes screenshots gets around to reading the clock.
const preflopShove = () => allInRunout({
  board: [],
  leaders: [1],
  odds: [
    { seat: 0, equity_permille: 312, outs: ["Kh", "Qh", "Jh", "Th", "9h", "8h", "6h", "5h", "4h"] },
    { seat: 1, equity_permille: 688, outs: [] },
  ],
});

test("shows the bots' cards mid-hand when the server sends them", async ({ page }) => {
  // The x-ray is an account option the server enforces; the client's job is
  // simply to draw whatever hole cards arrive, betting still in progress.
  await mountTable(page, {
    ...tableState,
    hand: {
      ...tableState.hand,
      seats: [
        { index: 1, hole_cards: ["Ac", "Ad"] },
        { index: 5, hole_cards: ["7h", "Td"] },
      ],
    },
  });

  const mina = page.locator(".seat", { hasText: "Mina" });
  await expect(mina.locator(".seat-cards.revealed")).toBeVisible();
  await expect(mina.locator(".seat-cards .playing-card")).toHaveCount(2);
  await expect(page.locator(".seat", { hasText: "Jo" }).locator(".seat-cards.revealed")).toBeVisible();
  // Nothing has been settled by looking: it is still somebody's turn.
  await expect(page.locator(".seat.winner")).toHaveCount(0);
  // A seat the server said nothing about stays face down.
  await expect(page.locator(".seat", { hasText: "Sam" }).locator(".seat-cards.revealed")).toHaveCount(0);
});

test("runs an all-in board out one street at a time", async ({ page }) => {
  await mountTable(page, preflopShove());
  const board = page.locator(".board .playing-card:not(.slot-card)");
  const result = page.locator(".showdown-result");

  // Betting closed before the flop, so nothing is out yet -- but the hands are
  // face up, so somebody is already ahead.
  await expect(board).toHaveCount(0);
  await expect(page.locator(".seat-cards.revealed")).toHaveCount(2);
  await expect(page.locator(".seat.leading")).toHaveCount(1);
  await expect(page.locator(".seat.leading")).toContainText("Mina");
  await expect(page.locator(".showdown-odds")).toContainText("31.2%");
  await expect(result).toHaveText("");
  const revealLayout = await page.locator(".table-stage").evaluate((stage) => {
    const odds = stage.querySelector(".showdown-odds").getBoundingClientRect();
    const oddsItems = [...stage.querySelectorAll(".showdown-odds span")].map((node) => node.getBoundingClientRect());
    const viewer = stage.querySelector(".seat.viewer").getBoundingClientRect();
    const boardBox = stage.querySelector(".board").getBoundingClientRect();
    const rowCount = new Set(oddsItems.map((item) => Math.round(item.top))).size;
    const metrics = stage.querySelector(".table-metrics").getBoundingClientRect();
    return {
      rowCount,
      itemCount: oddsItems.length,
      clearsViewer: odds.bottom <= viewer.top || odds.top >= viewer.bottom,
      boardGap: viewer.top - boardBox.bottom,
      oddsRightOfBoard: odds.left >= boardBox.right,
      boardCentred: Math.abs((boardBox.left - metrics.right) - (odds.left - boardBox.right)),
      odds: { top: odds.top, bottom: odds.bottom },
      board: { top: boardBox.top, bottom: boardBox.bottom },
      viewer: { top: viewer.top, bottom: viewer.bottom },
    };
  });
  if ((page.viewportSize()?.width || 0) <= 640) {
    // The phone stacks one box per player inside the reserved right rail.
    expect(revealLayout.rowCount, `V37: rail odds get a row each ${JSON.stringify(revealLayout)}`).toBe(revealLayout.itemCount);
    expect(revealLayout.oddsRightOfBoard, `V37: rail odds sit right of the shared cards ${JSON.stringify(revealLayout)}`).toBe(true);
    expect(revealLayout.boardCentred, `V37: shared cards centre between metrics and rail ${JSON.stringify(revealLayout)}`).toBeLessThanOrEqual(8);
  } else {
    expect(revealLayout.rowCount, `V37: odds must stay on one row ${JSON.stringify(revealLayout)}`).toBe(1);
  }
  expect(revealLayout.clearsViewer, `V37: odds must not overlap the viewer seat ${JSON.stringify(revealLayout)}`).toBe(true);
  expect(revealLayout.boardGap, `V37: center content must keep a visible viewer gap ${JSON.stringify(revealLayout)}`).toBeGreaterThanOrEqual(12);
  await expectLayout(page, "allin-reveal-table", TABLE_LAYOUT);
  await expectImage(page, "allin-reveal-table.png", { fullPage: true });
  // Nothing may give the ending away while the board is still coming: there is
  // no result to leak, because the server has not computed one (SPEC V59).
  await expect(page.locator(".seat.winner")).toHaveCount(0);
  await expect(page.locator(".game-log")).not.toContainText("wins");
  // Nor may a balance settle early. Both players are all in, so every chip is
  // in the pot and nothing is in front of anybody; Mina takes the $400 only
  // once the river lands.
  const mina = page.locator(".seat", { hasText: "Mina" }).locator(".seat-stack");
  await expect(mina).toHaveText("$0");
  await expect(page.locator(".table-metrics")).toContainText("$400");
  // A seated player may turn the next card; the server's deadline turns it
  // anyway, so it counts down like the next hand does and a press only ever
  // brings it forward.
  // Remounted with a fresh deadline: the layout pass and the screenshots above
  // spend real seconds, and this is about how the clock reads when it starts.
  await mountTable(page, preflopShove());
  await expect(page.locator(".showdown-advance button")).toContainText("Next card · 5s");
  await expect(page.locator(".showdown-advance .showdown-progress")).toBeVisible();

  // Each street arrives as its own state, and the leader is called out.
  await mountTable(page, allInRunout({
    board: ["Ah", "7c", "2s"],
    leaders: [0],
    odds: [
      { seat: 0, equity_permille: 712, outs: [] },
      { seat: 1, equity_permille: 288, outs: ["Ac", "Ad"] },
    ],
  }));
  await expect(board).toHaveCount(3);
  await expect(page.locator(".seat.leading")).toHaveCount(1);
  await expect(page.locator(".seat.leading .leading-role")).toHaveText("AHEAD");
  await expect(page.locator(".seat.leading")).toHaveCSS("border-top-color", "rgb(127, 212, 168)");
  await expect(result, "the result waits for the last card").toHaveText("");

  await mountTable(page, allInRunout({
    board: ["Ah", "7c", "2s", "7d"],
    leaders: [0],
    odds: [
      { seat: 0, equity_permille: 844, outs: [] },
      { seat: 1, equity_permille: 156, outs: ["Ac", "Ad"] },
    ],
  }));
  await expect(board).toHaveCount(4);
  await expect(result).toHaveText("");

  // The river settles the hand: it and the result it decided are read together
  // in the familiar post-hand pause, which is the board's moment.
  const settledSeats = structuredClone(showdownState.seats);
  settledSeats[1].stack = 62_400;
  await mountTable(page, {
    ...showdownState,
    seats: settledSeats,
    result_pause_seconds: 6,
    next_hand_at: new Date(Date.now() + 6_000).toISOString(),
  });
  await expect(board).toHaveCount(5);
  await expect(result).toContainText("Mina wins $400");
  await expect(page.locator(".seat.winner")).toHaveCount(1);
  await expect(page.locator(".game-log")).toContainText("Mina wins $400");
  await expect(page.locator(".showdown-advance button")).toContainText("OK · ");
  await expect(mina).toHaveText("$624");
  // A settled hand has a winner, not a leader: AHEAD must not sit beside it.
  await expect(page.locator(".seat.leading")).toHaveCount(0);
  await expect(page.locator(".leading-role")).toHaveCount(0);
});

test("uses the short acknowledgement window for a fold result", async ({ page }) => {
  await mountTable(page, foldResultState);
  await expect(page.locator(".showdown-advance button")).toContainText("OK · 3s");
});

test("picks any legal raise from the custom wager slider", async ({ page }) => {
  const posts = [];
  await page.route("**/tables/mock/action", async (route) => {
    posts.push(route.request().postDataJSON());
    await route.fulfill({ json: { ok: true } });
  });
  await mountTable(page, tableState);

  // Every raise, preset or slider, sits left of All In, which closes the row.
  const edge = page.locator(".action-edge-right button");
  await expect(edge).toHaveText(["Raise\u2026", "All In"]);
  await page.getByRole("button", { name: "Raise a custom amount" }).click();

  const slider = page.getByRole("slider", { name: "Raise amount" });
  const confirm = page.locator(".wager-confirm");
  await expect(page.locator(".wager-dialog output")).toHaveText("$36");
  await expect(confirm).toHaveText("$36");
  // Both ends of the legal range are reachable: the top rung is the shove.
  await expect(page.locator(".wager-range span")).toHaveText(["$36", "$166"]);

  await slider.fill("30");
  await expect(confirm).toHaveText("$66");
  await slider.fill("130");
  await expect(confirm).toHaveText("$166");

  await confirm.click();
  await expect(page.locator(".wager-dialog")).toHaveCount(0);
  expect(posts, "the slider submits the chips the raise adds, not the street total").toEqual([{ kind: "raise", amount: 15_400 }]);
});


// A hand ending is when the buttons under the table change, so it is exactly
// when the table must not move: the seats hold the space their cards had.
test("keeps the seats the same height once the cards are gone", async ({ page }) => {
  const read = async () => page.locator(".table-shell").evaluate((shell) => {
    const box = (selector) => shell.querySelector(selector).getBoundingClientRect();
    return {
      decisionTop: box(".decision-area").top,
      stageHeight: box(".table-stage").height,
      opponentHeight: box(".seat:not(.viewer)").height,
      viewerHeight: box(".seat.viewer").height,
    };
  });
  await mountTable(page, tableState);
  const live = await read();
  await mountTable(page, foldResultState);
  const between = await read();
  expect(between, `V53: the table must not shrink when a hand ends ${JSON.stringify({ live, between })}`).toEqual(live);
});

/**
 * The three shapes the panel takes in one session: a live hand, the gap between
 * hands, and a result that pays you. Every one of them used to move it — the
 * empty hand narrowed the card column, a longer stack widened the figures
 * beside it, and both re-centred the panel; the metrics leaving the felt and a
 * showdown's bigger opponent cards moved it up and down the screen as well.
 */
test("keeps your own hand in one place as cards and winnings come and go", async ({ page }) => {
  const betweenHands = { ...tableState, hand: null, next_hand_at: "2099-01-01T00:00:00Z" };
  const paidOut = {
    ...betweenHands,
    seats: tableState.seats.map((seat) => (seat.index === 2 ? { ...seat, stack: 106_600 } : seat)),
    last_hand: {
      board: ["Ah", "7c", "2s", "7d", "As"],
      results: [{ seat: 2, hand: { label: "Two pair, aces and sevens" } }, { seat: 1, hand: { label: "Pair of aces" } }],
      awards: [{ seat: 2, amount: 90_000 }],
      contributions: { 1: 45_000, 2: 45_000 },
      revealed_hole_cards: [[1, ["Kh", "Qh"]], [2, ["5c", "6c"]]],
      events: [{ street: "Complete", seat: 2, kind: "Award", amount: 90_000 }],
    },
  };
  const read = async () => page.locator(".table-shell").evaluate((shell) => {
    const box = (selector) => {
      const rect = shell.querySelector(selector).getBoundingClientRect();
      return { x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height) };
    };
    return {
      panel: box(".seat.viewer"),
      hand: box(".seat.viewer .seat-cards"),
      summary: box(".seat.viewer .viewer-summary"),
      decisionTop: Math.round(box(".decision-area").y),
    };
  });
  await mountTable(page, tableState);
  const live = await read();
  await mountTable(page, betweenHands);
  const between = await read();
  await mountTable(page, paidOut);
  const won = await read();
  expect(between, `V53: an empty hand must not move your own panel ${JSON.stringify({ live, between })}`).toEqual(live);
  expect(won, `V53: a result that pays you must not move your own panel ${JSON.stringify({ live, won })}`).toEqual(live);
});
