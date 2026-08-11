import { expect, test } from "@playwright/test";

const tableState = {
  id: "mock",
  name: "Friday Night Hold'em",
  stakes: { NoLimit: { small_blind: 100, big_blind: 200 } },
  button: 5,
  viewer_seat: 2,
  buy_in: 20_000,
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

async function mountTable(page, state) {
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
    main.insertAdjacentHTML("beforeend", '<div id="table-app" data-table-id="mock"></div>');
  });
  await page.evaluate(() => import(`/public/table.js?e2e=${Date.now()}`));
}

test("offers six concise game presets", async ({ page }) => {
  await page.goto("/tables/new");
  await expect(page.locator(".setup-option")).toHaveCount(6);
  await expect(page).toHaveScreenshot("game-setup.png", { fullPage: true });
});

test("shows live hand cues and event log", async ({ page }) => {
  await mountTable(page, tableState);
  await expect(page.locator(".game-log")).toBeVisible();
  await expect(page.locator(".seat.viewer .seat-cards .playing-card")).toHaveCount(2);
  const viewerCard = page.locator(".seat.viewer .seat-cards .playing-card").first();
  const secondViewerCard = page.locator(".seat.viewer .seat-cards .playing-card").nth(1);
  expect(await viewerCard.locator(".card-corner b").first().evaluate((rank) => getComputedStyle(rank).color)).toBe("rgb(32, 35, 31)");
  const configButtonBox = await page.getByRole("button", { name: "Card display settings" }).boundingBox();
  expect(configButtonBox.x).toBeGreaterThan((page.viewportSize()?.width || 0) * 0.75);
  expect(configButtonBox.y).toBeLessThan(80);
  await expect(page.locator(".card-config-dialog")).not.toBeVisible();
  await page.getByRole("button", { name: "Card display settings" }).click();
  await expect(page.locator(".card-config-dialog")).toBeVisible();
  const sizeSlider = page.locator('input[name="card-scale"]');
  const rankSlider = page.locator('input[name="rank-scale"]');
  const weightSlider = page.locator('input[name="rank-weight"]');
  await Promise.all([sizeSlider, rankSlider, weightSlider].map(async (slider) => {
    await expect(slider).toHaveValue("100");
    await expect(slider).toHaveAttribute("min", "50");
    await expect(slider).toHaveAttribute("max", "200");
  }));
  await expect(page.locator(".card-config-preview .playing-card")).toHaveCount(2);
  const previewBox = await page.locator(".card-config-preview .playing-card").first().boundingBox();
  const liveBox = await viewerCard.boundingBox();
  expect(Math.abs(previewBox.width - liveBox.width)).toBeLessThan(1);
  expect(Math.abs(previewBox.height - liveBox.height)).toBeLessThan(1);
  await expect(page.locator(".card-config-dialog")).toHaveScreenshot("card-config-dialog.png");
  await sizeSlider.fill("50");
  const initialCardBox = await viewerCard.boundingBox();
  await sizeSlider.fill("100");
  await expect(page.locator(".card-config-dialog output").first()).toHaveText("100%");
  const enlargedCardBox = await viewerCard.boundingBox();
  expect(enlargedCardBox.width).toBeGreaterThan(initialCardBox.width * 1.8);
  expect(await page.evaluate(() => localStorage.getItem("table-card-size-percent"))).toBe("100");
  await rankSlider.fill("50");
  await weightSlider.fill("50");
  const initialRank = await viewerCard.locator(".card-corner b").first().evaluate((rank) => ({ size: parseFloat(getComputedStyle(rank).fontSize), weight: Number(getComputedStyle(rank).fontWeight) }));
  await rankSlider.fill("100");
  await weightSlider.fill("100");
  const tunedRank = await viewerCard.locator(".card-corner b").first().evaluate((rank) => ({ size: parseFloat(getComputedStyle(rank).fontSize), weight: Number(getComputedStyle(rank).fontWeight) }));
  expect(tunedRank.size).toBeGreaterThan(initialRank.size);
  expect(tunedRank.weight).toBeGreaterThan(initialRank.weight);
  expect(tunedRank.weight).toBe(900);
  expect(await page.evaluate(() => localStorage.getItem("table-rank-size-percent"))).toBe("100");
  expect(await page.evaluate(() => localStorage.getItem("table-rank-weight-percent"))).toBe("100");
  await page.getByRole("button", { name: "Close" }).click();
  const firstBeforeMagnify = await viewerCard.boundingBox();
  const secondBeforeMagnify = await secondViewerCard.boundingBox();
  await viewerCard.hover();
  await page.waitForTimeout(250);
  const firstMagnified = await viewerCard.boundingBox();
  const secondMagnified = await secondViewerCard.boundingBox();
  expect(firstMagnified.width).toBeGreaterThan(firstBeforeMagnify.width * 1.15);
  expect(secondMagnified.width).toBeGreaterThan(secondBeforeMagnify.width * 1.15);
  await page.locator(".brand").hover();
  await page.waitForTimeout(250);
  await expect(page.locator(".empty-seat")).toHaveCount(0);
  await expect(page.locator(".board .empty-card")).toHaveCount(0);
  await expect(page.locator(".actions input")).toHaveCount(0);
  await expect(page.locator(".actions button")).toHaveText(["Fold", "Call $12", "$24", "$36", "$38", "$76", "All In"]);
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
  await expect(page.locator(".seat-wager")).toHaveCount(3);
  await expect(page.locator(".seat.viewer .seat-wager")).toHaveText("$12");
  const viewerWager = await page.locator(".seat.viewer .seat-wager").boundingBox();
  const viewerCards = await page.locator(".seat.viewer .seat-cards").boundingBox();
  expect(viewerWager.y + viewerWager.height, "V16: viewer wager must sit above viewer cards").toBeLessThanOrEqual(viewerCards.y + 1);
  const tableStatus = await page.locator(".table-status").boundingBox();
  const wagerOverlapsStatus = viewerWager.x < tableStatus.x + tableStatus.width && viewerWager.x + viewerWager.width > tableStatus.x && viewerWager.y < tableStatus.y + tableStatus.height && viewerWager.y + viewerWager.height > tableStatus.y;
  expect(wagerOverlapsStatus, "V16: viewer wager must not cover table status").toBe(false);
  await expect(page.locator(".seat.folded")).toHaveCount(1);
  await expect(page.locator(".seat.all-in")).toHaveCount(1);
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
  expect(await page.locator(".table-shell").innerText()).not.toMatch(/\$-?\d+\.\d{2}/);
  if ((page.viewportSize()?.width || 0) > 640) {
    const geometry = await page.locator(".table-stage").evaluate((stage) => ({
      stageHeight: stage.getBoundingClientRect().height,
      radius: getComputedStyle(stage.querySelector(".felt")).borderRadius,
    }));
    expect(geometry.stageHeight).toBeLessThan(550);
    expect(geometry.radius).not.toContain("%");
  }
  await expect(page).toHaveScreenshot("live-table.png", { fullPage: true });
});

test("keeps a top-rail tooltip inside a narrow desktop viewport", async ({ page }) => {
  await page.setViewportSize({ width: 702, height: 832 });
  await mountTable(page, tableState);
  const playerInfo = page.locator(".seat.tooltip-below.tooltip-right .player-info").filter({ hasText: "Sam" });
  await expect(playerInfo).toHaveCount(1);
  await playerInfo.hover();
  const tooltipBox = await playerInfo.locator(".player-tooltip").boundingBox();
  expect(tooltipBox.x, "V20: narrow top-rail tooltip left edge must remain visible").toBeGreaterThanOrEqual(0);
  expect(tooltipBox.y, "V20: narrow top-rail tooltip top edge must remain visible").toBeGreaterThanOrEqual(0);
  expect(tooltipBox.x + tooltipBox.width, "V20: narrow top-rail tooltip right edge must remain visible").toBeLessThanOrEqual(702);
  expect(tooltipBox.y + tooltipBox.height, "V20: narrow top-rail tooltip bottom edge must remain visible").toBeLessThanOrEqual(832);
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
  await expect(page.locator(".showdown-result")).toContainText("Mina wins $400");
  await expect(page.locator(".game-log")).toContainText("Mina wins $400");
  const opponentCard = page.locator(".seat:not(.viewer) .seat-cards.revealed .playing-card").first();
  expect((await opponentCard.boundingBox()).width).toBeGreaterThan(40);
  await expect(page.locator(".seat.winner")).toHaveCSS("border-top-width", "2px");
  await expect(page.locator(".showdown-advance button")).toContainText("OK · 6s");
  await expect(page.locator(".showdown-progress")).toHaveCSS("width", /.+/);
  await expect(page.locator(".last-hand")).toHaveCount(0);
  await expect(page).toHaveScreenshot("showdown-table.png", { fullPage: true });
  await page.locator(".showdown-advance button").click();
  expect(continued).toBe(true);
});

test("uses the short acknowledgement window for a fold result", async ({ page }) => {
  await mountTable(page, foldResultState);
  await expect(page.locator(".showdown-advance button")).toContainText("OK · 3s");
});
