import { expect, test } from "@playwright/test";

const tableState = {
  id: "mock",
  name: "Friday Night Hold'em",
  stakes: { NoLimit: { small_blind: 100, big_blind: 200 } },
  button: 5,
  viewer_seat: 2,
  viewer_leaving: false,
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
  expect(enlargedCardBox.width).toBeGreaterThan(initialCardBox.width * 1.8);
  expect(await page.evaluate(() => localStorage.getItem("table-card-size-percent"))).toBe("100");
  const rankWeightVariable = () => page.evaluate(() => document.documentElement.style.getPropertyValue("--card-rank-weight"));
  await rankSlider.fill("50");
  await weightSlider.fill("50");
  await expect.poll(rankWeightVariable).toBe("450");
  const initialRank = await viewerCard.locator(".card-corner").first().evaluate((corner) => ({ size: parseFloat(getComputedStyle(corner.querySelector("b")).fontSize), suitSize: parseFloat(getComputedStyle(corner.querySelector("i")).fontSize), weight: Number(getComputedStyle(corner.querySelector("b")).fontWeight) }));
  await rankSlider.fill("100");
  await weightSlider.fill("100");
  await expect.poll(rankWeightVariable).toBe("900");
  const tunedRank = await viewerCard.locator(".card-corner").first().evaluate((corner) => ({ size: parseFloat(getComputedStyle(corner.querySelector("b")).fontSize), suitSize: parseFloat(getComputedStyle(corner.querySelector("i")).fontSize), weight: Number(getComputedStyle(corner.querySelector("b")).fontWeight) }));
  expect(tunedRank.size).toBeGreaterThan(initialRank.size);
  expect(tunedRank.suitSize).toBeGreaterThan(initialRank.suitSize);
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
  // Wager buttons name the street total they raise to, so they never read the same as the call.
  await expect(page.locator(".actions button")).toHaveText(["Fold", "Call $12", "Raise $36", "Raise $48", "Raise $50", "Raise $88", "All In"]);
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
  // Every seat keeps a wager slot so it does not resize when a bet lands.
  await expect(page.locator(".seat-wager")).toHaveCount(6);
  await expect(page.locator(".seat-wager:not(.no-wager)")).toHaveCount(3);
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

test("keeps the table log footprint stable as events accumulate", async ({ page }) => {
  await mountTable(page, tableState);
  const log = page.locator(".game-log");
  await log.locator("ol").evaluate((list) => list.replaceChildren(list.firstElementChild));
  const sparse = await log.evaluate((node) => ({ height: node.getBoundingClientRect().height, controlsTop: document.querySelector(".table-controls").getBoundingClientRect().top }));
  await log.locator("ol").evaluate((list) => {
    const row = list.firstElementChild;
    for (let index = 0; index < 30; index += 1) list.append(row.cloneNode(true));
  });
  const dense = await log.evaluate((node) => ({ height: node.getBoundingClientRect().height, controlsTop: document.querySelector(".table-controls").getBoundingClientRect().top, scrolls: node.scrollHeight > node.clientHeight }));
  expect(dense.height, "V22: table log height must not grow with events").toBe(sparse.height);
  expect(dense.controlsTop, "V22: content below table log must remain fixed").toBe(sparse.controlsTop);
  expect(dense.scrolls, "V22: excess table events must scroll inside the fixed log").toBe(true);
});

test("offers one state-aware table lifecycle command", async ({ page }) => {
  await mountTable(page, tableState);
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Leave"]);
  await expect(page.getByRole("button", { name: "Sit out" })).toHaveCount(0);

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
  // Seating a bot is a single control beside the lifecycle command.
  await expect(page.locator(".seat-bot-dialog")).not.toBeVisible();
  await page.getByRole("button", { name: "Seat a bot" }).click();
  await expect(page.locator(".seat-bot-dialog")).toBeVisible();
  await expect(page.locator('.seat-bot-dialog select[name="seat"] option')).toHaveText(["Seat 2"]);
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.locator(".seat-bot-dialog")).not.toBeVisible();
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
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Re-Buy In $200"]);
  await page.locator(".table-controls .table-command").click();
  expect(rebuyBody).toEqual({});

  await mountTable(page, { ...tableState, viewer_leaving: true });
  await expect(page.locator(".table-controls .table-command")).toHaveText(["Leaving..."]);
  await expect(page.locator(".table-controls .table-command")).toBeDisabled();
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
    const viewerCards = stage.querySelector(".seat.viewer .seat-cards").getBoundingClientRect();
    const viewerWager = stage.querySelector(".seat.viewer .seat-wager")?.getBoundingClientRect();
    const tableCenter = stage.querySelector(".table-center").getBoundingClientRect();
    const card = stage.querySelector(".seat.viewer .playing-card");
    const corners = [...card.querySelectorAll(".card-corner b, .card-corner i")].map((node) => node.getBoundingClientRect());
    const centerContent = [...card.querySelectorAll(".pip-grid i, .card-art")].map((node) => node.getBoundingClientRect());
    // Browser-evaluated helpers cannot close over test-scope functions.
    // oxlint-disable-next-line unicorn/consistent-function-scoping
    const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    return {
      centerOverlap: overlaps(viewerCards, tableCenter),
      wagerOverlap: viewerWager ? overlaps(viewerWager, tableCenter) : false,
      faceOverlap: corners.some((corner) => centerContent.some((content) => overlaps(corner, content))),
      pipOverlap: centerContent.some((pip, index) => centerContent.slice(index + 1).some((other) => overlaps(pip, other))),
      viewerCards: { top: viewerCards.top, bottom: viewerCards.bottom },
      tableCenter: { top: tableCenter.top, bottom: tableCenter.bottom },
      corners: corners.map(({ top, right, bottom, left }) => ({ top, right, bottom, left })),
      centerContent: centerContent.map(({ top, right, bottom, left }) => ({ top, right, bottom, left })),
    };
  });
  expect(geometry.centerOverlap, `V21: max-size viewer cards must clear table center content ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.wagerOverlap, `V21: max-size viewer wager must clear table center content ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.faceOverlap, `V21: max-size ranks must clear pips and center art ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.pipOverlap, `V21: reflowed pips must remain distinct ${JSON.stringify(geometry)}`).toBe(false);
  await expect(page).toHaveScreenshot("max-card-table.png", { fullPage: true });
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

test("packs the table into a landscape phone without scrolling", async ({ page }) => {
  await page.setViewportSize({ width: 932, height: 430 });
  await mountTable(page, tableState);
  await expect(page.locator(".seat.viewer")).toBeVisible();
  const rail = await page.locator(".table-stage").evaluate((stage) => {
    const seats = [...stage.querySelectorAll(".seat")];
    const viewer = stage.querySelector(".seat.viewer").getBoundingClientRect();
    const others = seats.filter((seat) => !seat.classList.contains("viewer")).map((seat) => seat.getBoundingClientRect());
    const board = [...stage.querySelectorAll(".board .playing-card, .table-metrics")].map((node) => node.getBoundingClientRect());
    // Browser-evaluated helpers cannot close over test-scope functions.
    // oxlint-disable-next-line unicorn/consistent-function-scoping
    const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    return {
      rows: new Set(seats.map((seat) => Math.round(seat.getBoundingClientRect().top))).size,
      viewerWidth: viewer.width,
      otherWidth: Math.max(...others.map((rect) => rect.width)),
      boardOverlap: seats.some((seat) => board.some((card) => overlaps(seat.getBoundingClientRect(), card))),
      stageScrolls: stage.scrollHeight > stage.clientHeight,
    };
  });
  expect(rail.rows, "L1: the landscape rail must stay on one row").toBe(1);
  expect(rail.viewerWidth, "L2: the viewer seat must be about twice its neighbours").toBeGreaterThan(rail.otherWidth * 1.8);
  expect(rail.boardOverlap, "L3: the rail must not collide with the board").toBe(false);
  expect(rail.stageScrolls, "L4: a landscape table must fit its stage").toBe(false);
  expect(await page.evaluate(() => document.documentElement.scrollHeight <= document.documentElement.clientHeight)).toBe(true);
  await expect(page).toHaveScreenshot("landscape-table.png", { fullPage: true });
});

test("uses the short acknowledgement window for a fold result", async ({ page }) => {
  await mountTable(page, foldResultState);
  await expect(page.locator(".showdown-advance button")).toContainText("OK · 3s");
});
