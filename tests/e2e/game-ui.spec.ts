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
  // You may only pick an entry you can actually cover.
  await expect(page.locator('.setup-option[value="100000"]')).toBeEnabled();
  await expect(page.locator('.setup-option[value="1000000"]')).toBeDisabled();
  await expect(page.locator('.setup-option[value="1000000"]')).toContainText("More than you have");

  await page.locator('.setup-option[value="100000"]').click();
  await expect(page.locator("#setup-summary")).toHaveText("$1,000 tournament · 9 players · 10,000 chips · top 3 paid");
  await page.fill('input[name="name"]', "Sunday deep");
  await page.locator(".setup-create").click();
  await page.waitForURL(/\/tables\/[0-9a-f-]+$/);
  await expect(page.locator(".tournament-panel")).toContainText("Level 1");
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
  const wagerBehindCards = viewerWager.x < viewerCards.x + viewerCards.width
    && viewerWager.x + viewerWager.width > viewerCards.x
    && viewerWager.y < viewerCards.y + viewerCards.height
    && viewerWager.y + viewerWager.height > viewerCards.y;
  expect(wagerBehindCards, "V16: viewer cards must not cover the viewer wager").toBe(false);
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
  // The shell is locked to the viewport at every size, so the page never scrolls.
  expect(await page.evaluate(() => document.documentElement.scrollHeight <= document.documentElement.clientHeight), "V23: the table must not scroll the page").toBe(true);
  expect(await page.locator(".table-shell").innerText()).not.toMatch(/\$-?\d+\.\d{2}/);
  if ((page.viewportSize()?.width || 0) > 640) {
    const geometry = await page.locator(".table-stage").evaluate((stage) => ({
      stageHeight: stage.getBoundingClientRect().height,
      viewport: document.documentElement.clientHeight,
      radius: getComputedStyle(stage.querySelector(".felt")).borderRadius,
      statusOverlap: [...stage.parentElement.querySelectorAll(".seat")].some((seat) => {
        const status = stage.parentElement.querySelector(".table-status")?.getBoundingClientRect();
        if (!status) return false;
        const rect = seat.getBoundingClientRect();
        return rect.left < status.right && rect.right > status.left && rect.top < status.bottom && rect.bottom > status.top;
      }),
    }));
    expect(geometry.stageHeight).toBeLessThanOrEqual(geometry.viewport);
    expect(geometry.statusOverlap, "V24: seats must not cover the table status").toBe(false);
    expect(geometry.radius).not.toContain("%");
  }
  await expect(page).toHaveScreenshot("live-table.png", { fullPage: true });
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
      const rail = [...stage.querySelectorAll(".seat, .seat-cards")];
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
    /* oxlint-enable no-await-in-loop */
  }
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
  // And it waits to be asked before it plays.
  await expect(page.getByRole("button", { name: "Deal a hand" })).toBeVisible();

  // A table full of people has no room, and offers nothing.
  const packed = {
    ...houseTable,
    can_deal: false,
    seats: houseTable.seats.map((seat) => Object.assign({}, seat, { occupant: "human", bot: false })),
  };
  await mountTable(page, packed);
  await expect(page.locator(".table-controls .table-command")).toHaveCount(0);
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
    const cardBox = card.getBoundingClientRect();
    // Browser-evaluated helpers cannot close over test-scope functions.
    // oxlint-disable-next-line unicorn/consistent-function-scoping
    const overlaps = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    return {
      centerOverlap: overlaps(viewerCards, tableCenter),
      wagerOverlap: viewerWager ? overlaps(viewerWager, tableCenter) : false,
      faceOverlap: corners.some((corner, index) => corners.slice(index + 1).some((other) => overlaps(corner, other))),
      faceEscapesCard: corners.some((corner) => corner.top < cardBox.top || corner.bottom > cardBox.bottom || corner.left < cardBox.left || corner.right > cardBox.right),
      viewerCards: { top: viewerCards.top, bottom: viewerCards.bottom },
      tableCenter: { top: tableCenter.top, bottom: tableCenter.bottom },
      corners: corners.map(({ top, right, bottom, left }) => ({ top, right, bottom, left })),
    };
  });
  expect(geometry.centerOverlap, `V21: max-size viewer cards must clear table center content ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.wagerOverlap, `V21: max-size viewer wager must clear table center content ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.faceOverlap, `V21: max-size rank and suit must not collide ${JSON.stringify(geometry)}`).toBe(false);
  expect(geometry.faceEscapesCard, `V21: max-size rank and suit must stay on the card ${JSON.stringify(geometry)}`).toBe(false);
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

test("runs an all-in board out one street at a time", async ({ page }) => {
  const allIn = {
    ...showdownState,
    // Seat 1 has already been paid the $400 pot, the way a settled hand leaves it.
    seats: showdownState.seats.map((seat) => (seat.index === 1 ? { ...seat, stack: 62_400 } : seat)),
    result_pause_seconds: 21,
    next_hand_at: new Date(Date.now() + 21_000).toISOString(),
    last_hand: {
      ...showdownState.last_hand,
      runout_from: 0,
      runout: [
        { cards: 3, leaders: [0] },
        { cards: 4, leaders: [0] },
        { cards: 5, leaders: [1] },
      ],
    },
  };
  await mountTable(page, allIn);
  const board = page.locator(".board .playing-card");
  const result = page.locator(".showdown-result");

  // Betting closed before the flop, so nothing is out yet.
  await expect(board).toHaveCount(0);
  await expect(page.locator(".seat.leading")).toHaveCount(0);
  await expect(result).toHaveText("");
  // Nothing may give the ending away while the board is still coming.
  await expect(page.locator(".seat.winner")).toHaveCount(0);
  await expect(page.locator(".game-log")).not.toContainText("wins");
  // The reveal is not optional: there is nothing to press until it finishes.
  await expect(page.locator(".showdown-advance button")).toHaveCount(0);
  await expect(page.locator(".showdown-advance.spectator")).toBeVisible();
  // Nor may a balance settle early; Mina takes $400 only once the river lands.
  const mina = page.locator(".seat", { hasText: "Mina" }).locator(".seat-stack");
  await expect(mina).toHaveText("$224");

  // Each street lands five seconds apart, and the leader is called out.
  await page.clock.install();
  await page.clock.fastForward(5_100);
  await expect(board).toHaveCount(3);
  await expect(page.locator(".seat.leading")).toHaveCount(1);
  await expect(page.locator(".seat.leading .leading-role")).toHaveText("AHEAD");
  await expect(result, "the result waits for the last card").toHaveText("");

  await page.clock.fastForward(5_000);
  await expect(board).toHaveCount(4);

  await page.clock.fastForward(5_000);
  await expect(board).toHaveCount(5);
  // The river changes who is ahead, and only now does the result read out.
  await expect(page.locator(".seat.leading")).toHaveCount(1);
  await expect(result).toContainText("Mina wins $400");
  await expect(page.locator(".seat.winner")).toHaveCount(1);
  await expect(page.locator(".game-log")).toContainText("Mina wins $400");
  // Once the last card is down, the acknowledgement and the pot both land.
  await expect(page.locator(".showdown-advance button")).toHaveCount(1);
  await expect(mina).toHaveText("$624");
});

test("uses the short acknowledgement window for a fold result", async ({ page }) => {
  await mountTable(page, foldResultState);
  await expect(page.locator(".showdown-advance button")).toContainText("OK · 3s");
});
