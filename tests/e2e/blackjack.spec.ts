import { IPHONE_LANDSCAPE, IPHONE_MAX_PORTRAIT, IPHONE_PORTRAIT, IPHONE_SE_PORTRAIT, useDevice } from "./devices";
import { expect, test } from "./fixtures";

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
  await expect(page.locator(".bank-widget")).not.toHaveAttribute("open", "");
  await expect(page.locator("#bank-panel .re-up-button")).toHaveCount(1);
  await expect(page.locator("#bank-panel")).toBeHidden();
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
  await page.locator(".brand").click();
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
  await expect(page.locator(".blackjack-play-area .playing-card").first()).toBeVisible();

  // The $10 leaves the bank on the deal, but roughly one hand in twenty is a
  // natural: it settles before this assertion can run and pays 3:2 straight
  // back, so a fixed $990 makes this test deal-dependent. What "refresh the
  // header balance" means is that the header agrees with the bank itself.
  // No assertion that the balance left $1,000: a natural against a dealer
  // natural pushes the bet straight back, and that is not a stale header.
  const { balance } = await (await page.request.get("/api/bank")).json();
  await expect(page.locator("#bank-balance")).toHaveText(`$${Math.round(balance / 100).toLocaleString("en-US")}`);
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

test("blackjack mobile uses the available card and action space", async ({ page }) => {
  await page.setViewportSize({ width: 412, height: 915 });
  await signIn(page, "blackjackmobile");
  await page.request.post("/api/bank", { data: {} });
  await page.route("**/blackjack/start", async (route) => {
    await route.fulfill({
      json: {
        id: "00000000-0000-0000-0000-000000000002",
        bet: 1000,
        player: ["2h", "3c"],
        dealer: ["As"],
        player_score: 5,
        dealer_score: null,
        status: "Playing",
        message: "Choose an action.",
        payout: 0,
        can_hit: true,
        can_stand: true,
        can_double: true,
        can_split: true,
        can_insure: true,
        insurance: 0,
        hands: [{ cards: ["2h", "3c"], bet: 1000, score: 5, status: "Active", blackjack: false }],
        active_hand: 0,
        settings: { decks: 8, penetration_percent: 50, counting_tutor: false, counting_quiz: false, bet_analyzer: false },
        count: null,
        trainer_log: [],
        quiz: null,
        analysis: [],
        shoe: { decks: 8, total_cards: 416, dealt_cards: 4, remaining_cards: 412, cut_card: 208, penetration_percent: 50, hands_dealt: 1, fresh_shuffle: true },
      },
    });
  });

  for (const device of [IPHONE_PORTRAIT, IPHONE_SE_PORTRAIT, IPHONE_MAX_PORTRAIT, IPHONE_LANDSCAPE]) {
    /* oxlint-disable no-await-in-loop */
    const viewport = device.viewport;
    await useDevice(page, device);
    await page.goto("/blackjack");
    await page.getByRole("button", { name: "Deal $10" }).click();
    await expect(page.locator(".blackjack-actions button")).toHaveText(["Hit", "Stand", "Double", "Split", "Insurance"]);

    const layout = await page.locator(".blackjack-table").evaluate((table) => {
      const tableBox = table.getBoundingClientRect();
      const playArea = table.querySelector(".blackjack-play-area").getBoundingClientRect();
      const hands = [...table.querySelectorAll(".blackjack-hand")].map((hand) => hand.getBoundingClientRect());
      const cards = [...table.querySelectorAll(".blackjack-play-area .playing-card")].map((card) => card.getBoundingClientRect());
      const buttons = [...table.querySelectorAll(".blackjack-actions button")].map((button) => {
        const rect = button.getBoundingClientRect();
        const styles = getComputedStyle(button);
        const actionBar = button.closest(".blackjack-actions").getBoundingClientRect();
        return {
          label: button.textContent,
          left: rect.left,
          right: rect.right,
          height: Math.round(rect.height),
          fontSize: styles.fontSize,
          scrollWidth: button.scrollWidth,
          clientWidth: button.clientWidth,
          scrollHeight: button.scrollHeight,
          clientHeight: button.clientHeight,
          insideTable: rect.left >= tableBox.left - 1 && rect.right <= tableBox.right + 1 && rect.top >= tableBox.top - 1 && rect.bottom <= tableBox.bottom + 1,
          barLeft: actionBar.left,
          barRight: actionBar.right,
        };
      });
      return {
        documentScrolls: document.documentElement.scrollHeight > document.documentElement.clientHeight,
        playAreaOpenSpace: playArea.height - hands.reduce((sum, hand) => sum + hand.height, 0),
        minCardWidth: Math.min(...cards.map((card) => card.width)),
        maxCardBottom: Math.max(...cards.map((card) => card.bottom)),
        playAreaBottom: playArea.bottom,
        buttons,
      };
    });
    expect(layout.documentScrolls, `V42: blackjack mobile must not scroll the document at ${JSON.stringify(viewport)}`).toBe(false);
    if (viewport.width <= 640) {
      expect(layout.minCardWidth, `V42: blackjack cards should use mobile space ${JSON.stringify(layout)}`).toBeGreaterThanOrEqual(90);
      expect(layout.playAreaOpenSpace, `V42: cards should not be tiny in a mostly empty play area ${JSON.stringify(layout)}`).toBeLessThan(260);
      expect(layout.maxCardBottom, "V42: cards must stay inside the play area").toBeLessThanOrEqual(layout.playAreaBottom + 1);
    }
    expect(new Set(layout.buttons.map((button) => button.height)).size, "V42: blackjack action buttons must share one height").toBe(1);
    expect(new Set(layout.buttons.map((button) => button.fontSize)).size, "V42: blackjack action buttons must share one font size").toBe(1);
    // 11px is the design system's type floor (--text-label); the old 11.2 was
    // the ad-hoc .7rem this row used before the scale existed. "Insurance" needs
    // 68px at the next step up and the button is 64px, so 11 is the real limit.
    expect(layout.buttons.every((button) => Number.parseFloat(button.fontSize) >= 11), `V46: blackjack action labels must remain readable at ${JSON.stringify(viewport)} ${JSON.stringify(layout.buttons)}`).toBe(true);
    expect(layout.buttons[0].left, "V44: blackjack actions must start at the bar edge").toBeLessThanOrEqual(layout.buttons[0].barLeft + 1);
    expect(layout.buttons.at(-1).right, "V44: blackjack actions must reach the bar edge").toBeGreaterThanOrEqual(layout.buttons.at(-1).barRight - 1);
    expect(layout.buttons.every((button) => button.insideTable), "V42: blackjack action buttons must stay in table bounds").toBe(true);
    expect(
      layout.buttons.every((button) => button.scrollWidth <= button.clientWidth && button.scrollHeight <= button.clientHeight),
      `V42: blackjack action labels must fit inside their buttons ${JSON.stringify({ viewport, buttons: layout.buttons })}`,
    ).toBe(true);
    /* oxlint-enable no-await-in-loop */
  }
});

// A hand keeps hitting until it stands or busts, and a split turns one hand
// into as many as four, so neither the card count nor the hand count is fixed
// when the stylesheet sizes them.
function longHandGame(hands) {
  const player = hands[0].cards;
  return {
    id: "00000000-0000-0000-0000-000000000003",
    bet: 1000,
    player,
    dealer: ["As"],
    player_score: hands[0].score,
    dealer_score: null,
    status: "Playing",
    message: "Choose an action.",
    payout: 0,
    can_hit: true,
    can_stand: true,
    can_double: false,
    can_split: false,
    can_insure: false,
    insurance: 0,
    hands,
    active_hand: 0,
    settings: { decks: 8, penetration_percent: 50, counting_tutor: false, counting_quiz: false, bet_analyzer: false },
    count: null,
    trainer_log: [],
    quiz: null,
    analysis: [],
    shoe: { decks: 8, total_cards: 416, dealt_cards: 9, remaining_cards: 407, cut_card: 208, penetration_percent: 50, hands_dealt: 1, fresh_shuffle: true },
  };
}

const LONG_HAND = [{ cards: ["2h", "3c", "4d", "2s", "3h", "2c", "5d"], bet: 1000, score: 21, status: "Active", blackjack: false }];
const SPLIT_HANDS = [
  { cards: ["8h", "5c", "4d"], bet: 1000, score: 17, status: "Active", blackjack: false },
  { cards: ["8d", "3c", "6d", "2h"], bet: 1000, score: 19, status: "Stand", blackjack: false },
  { cards: ["8s", "9c"], bet: 1000, score: 17, status: "Stand", blackjack: false },
  { cards: ["8c", "Td"], bet: 1000, score: 18, status: "Stand", blackjack: false },
];

for (const [name, hands] of [
  ["a seven-card hand", LONG_HAND],
  ["four split hands", SPLIT_HANDS],
] as const) {
  test(`blackjack keeps ${name} on screen`, async ({ page }) => {
    await signIn(page, "blackjacklonghand");
    await page.request.post("/api/bank", { data: {} });
    await page.route("**/blackjack/start", async (route) => {
      await route.fulfill({ json: longHandGame([...hands]) });
    });

    for (const device of [IPHONE_PORTRAIT, IPHONE_SE_PORTRAIT, IPHONE_MAX_PORTRAIT, IPHONE_LANDSCAPE]) {
      /* oxlint-disable no-await-in-loop */
      const viewport = device.viewport;
      await useDevice(page, device);
      await page.goto("/blackjack");
      await page.getByRole("button", { name: "Deal $10" }).click();
      // The dealer's hand plus one section per player hand.
      await expect(page.locator(".blackjack-play-area .blackjack-hand")).toHaveCount(hands.length + 1);

      const layout = await page.locator(".blackjack-play-area").evaluate((area) => {
        const areaBox = area.getBoundingClientRect();
        return {
          documentScrolls: document.documentElement.scrollHeight > document.documentElement.clientHeight,
          hands: [...area.querySelectorAll(".blackjack-hand")].map((hand) => {
            const handBox = hand.getBoundingClientRect();
            const cards = [...hand.querySelectorAll(".playing-card")].map((card) => card.getBoundingClientRect());
            return {
              cards: cards.length,
              width: Math.round(handBox.width),
              overflowLeft: Math.round(handBox.left - Math.min(...cards.map((card) => card.left))),
              overflowRight: Math.round(Math.max(...cards.map((card) => card.right)) - handBox.right),
              overflowBottom: Math.round(Math.max(...cards.map((card) => card.bottom)) - areaBox.bottom),
              minCardWidth: Math.round(Math.min(...cards.map((card) => card.width))),
            };
          }),
        };
      });

      expect(layout.documentScrolls, `V42: blackjack must not scroll the document at ${JSON.stringify(viewport)}`).toBe(false);
      expect(
        layout.hands.every((hand) => hand.overflowLeft <= 1 && hand.overflowRight <= 1),
        `V42: every card must stay inside its own hand ${JSON.stringify({ viewport, layout })}`,
      ).toBe(true);
      expect(
        layout.hands.every((hand) => hand.overflowBottom <= 1),
        `V42: every card must stay inside the play area ${JSON.stringify({ viewport, layout })}`,
      ).toBe(true);
      // Shrinking to fit is only a fix while the rank stays legible: a quarter
      // of the two-card width is the floor, which the collapsed split rows
      // (5px cards) missed by an order of magnitude.
      expect(
        layout.hands.every((hand) => hand.minCardWidth >= 24),
        `V42: blackjack cards must stay readable ${JSON.stringify({ viewport, layout })}`,
      ).toBe(true);
      /* oxlint-enable no-await-in-loop */
    }
  });
}
