import { IPHONE_LANDSCAPE, IPHONE_PORTRAIT, readChromeIntrusions, readClippedBoxes, readPageGutters, useDevice, type EmulatedDevice } from "./devices";
import { expect, test } from "./fixtures";

/**
 * V54 is a whole-app contract, not a table one. The poker table earned its
 * inset handling a bug at a time and every other surface was left on the plain
 * 1rem gutter, so a landscape phone put `Bet $25`, `Sit down` and `Pay off one
 * loan` behind the Dynamic Island while nothing scrolled and no image baseline
 * moved. These tests read the gutter each page reserves against the insets the
 * device actually has.
 */

let account = 0;
async function signIn(page, name: string) {
  account += 1;
  await page.goto("/");
  const suffix = `${Date.now()}${account}${Math.random().toString(36).slice(2, 7)}`;
  await page.fill('#register-form input[name="username"]', `${name}${suffix}`);
  await page.fill('#register-form input[name="display_name"]', name);
  await page.click("#register-form button");
  await page.waitForTimeout(300);
}

/** Every page reserves at least the inset on each edge it can be reached from. */
async function expectGuttersClearInsets(page, device: EmulatedDevice, where: string) {
  const [, right, bottom, left] = await readPageGutters(page);
  expect(left, `${where}: the left gutter must clear the device inset`).toBeGreaterThanOrEqual(device.insets.left);
  expect(right, `${where}: the right gutter must clear the device inset`).toBeGreaterThanOrEqual(device.insets.right);
  expect(bottom, `${where}: the bottom gutter must clear the home indicator`).toBeGreaterThanOrEqual(device.insets.bottom);
}

const PAGES = ["/", "/player", "/leaderboard", "/blackjack"];

test("every page reserves the phone's insets in both orientations", async ({ page }) => {
  test.skip((page.viewportSize()?.width || 0) > 640, "V54: only the phone project pins insets");
  await signIn(page, "Insets");
  /* oxlint-disable no-await-in-loop */
  for (const path of PAGES) {
    for (const device of [IPHONE_PORTRAIT, IPHONE_LANDSCAPE]) {
      await useDevice(page, device);
      await page.goto(path);
      await expect(page.locator(".page")).toBeVisible();
      await expectGuttersClearInsets(page, device, `${path} at ${device.viewport.width}x${device.viewport.height}`);
    }
  }
  /* oxlint-enable no-await-in-loop */
});

test("Hand Blitz keeps its fixed shell clear of the device chrome", async ({ page }) => {
  test.skip((page.viewportSize()?.width || 0) > 640, "V54: only the phone project pins insets");
  await signIn(page, "Blitz");
  await page.request.post("/api/bank", { data: {} });
  /* oxlint-disable no-await-in-loop */
  for (const device of [IPHONE_PORTRAIT, IPHONE_LANDSCAPE]) {
    await useDevice(page, device);
    await page.goto("/hand-blitz");
    await expect(page.locator(".blitz-shell")).toBeVisible();
    await expectGuttersClearInsets(page, device, `hand-blitz at ${device.viewport.width}x${device.viewport.height}`);
    const intrusions = await readChromeIntrusions(page, device);
    expect(intrusions, `V54: no Hand Blitz control may sit under the device chrome at ${device.viewport.width}x${device.viewport.height}`).toEqual([]);
  }
  /* oxlint-enable no-await-in-loop */
});

/**
 * Blackjack is one game per player now, so the phone could play it for real --
 * but a layout test that deals cards measures whatever the shoe happened to
 * give it. The layout is measured against mocked state instead: the same
 * island, the real stylesheet, the phone's own insets, and a hand that is
 * always the densest one the table can show.
 */
const seatedTable = {
  seated: true,
  id: "mock",
  bank_balance: 0,
  max_bets: [10_000, 20_000, 50_000, 100_000],
  affordable_max_bets: [10_000, 20_000, 50_000, 100_000],
  max_bet: 10_000,
  buy_in: 100_000,
  bet_options: [2_500, 5_000, 7_500, 10_000],
  min_bet: 2_500,
  phase: "betting",
  stack: 100_000,
  bet: null,
  insurance: 0,
  hands: [],
  current_hand: null,
  dealer: [],
  dealer_hidden: false,
  dealer_score: null,
  result: null,
  can_sit: false,
  can_leave: true,
  can_rebuy: false,
  can_bet: true,
  can_insure: false,
  can_decline: false,
  can_hit: false,
  can_stand: false,
  can_double: false,
  can_split: false,
  message: "Place your bet",
  shoe: { decks: 8, total_cards: 416, dealt_cards: 0, remaining_cards: 416, cut_card: 208, penetration_percent: 50, hands_dealt: 0, fresh_shuffle: false },
  trainer: { count: null, log: [], analysis: [], quiz: null },
  settings: { counting_tutor: false, counting_quiz: false, bet_analyzer: false },
  fresh_shuffle: false,
};

// The densest surface the table has: a split hand against a dealt dealer with
// every hand action live.
const splitTable = {
  ...seatedTable,
  phase: "playing",
  dealer: ["Ts"],
  dealer_hidden: true,
  dealer_score: null,
  current_hand: 0,
  stack: 90_000,
  bet: 2_500,
  hands: [
    { cards: ["8h", "3c"], score: 11, bet: 2_500, status: "Playing", blackjack: false },
    { cards: ["8d", "Kc", "2s"], score: 20, bet: 2_500, status: "Playing", blackjack: false },
  ],
  can_bet: false,
  can_hit: true,
  can_stand: true,
  can_double: true,
  can_split: true,
  message: "Your move",
};

// The slider before any of that: nothing is seated, so the page is the ladder.
const sitting = {
  ...seatedTable,
  seated: false,
  id: null,
  bank_balance: 250_000,
  stack: 0,
  can_sit: true,
  can_leave: false,
  can_bet: false,
  message: "Choose your maximum bet",
  shoe: null,
  trainer: null,
};

async function mountBlackjack(page, state) {
  await page.unroute("**/blackjack/state");
  await page.route("**/blackjack/state", (route) => route.fulfill({ json: state }));
  await page.goto("/blackjack");
  await expect(page.locator(".blackjack-table")).toBeVisible();
}

for (const [label, state, marker] of [["sitting down", sitting, /Sit down/], ["placing a bet", seatedTable, "Bet $25"], ["playing a split", splitTable, "Stand"]] as const) {
  test(`the blackjack table fits the phone while ${label}`, async ({ page }) => {
    test.skip((page.viewportSize()?.width || 0) > 640, "V54: only the phone project pins insets");
    await signIn(page, "BjPhone");
    await mountBlackjack(page, state);
    /* oxlint-disable no-await-in-loop */
    for (const device of [IPHONE_PORTRAIT, IPHONE_LANDSCAPE]) {
      const size = `${device.viewport.width}x${device.viewport.height}`;
      await useDevice(page, device);
      await expect(page.getByRole("button", { name: marker }).first()).toBeVisible();
      await expectGuttersClearInsets(page, device, `blackjack ${label} at ${size}`);
      const intrusions = await readChromeIntrusions(page, device);
      expect(intrusions, `V54: no blackjack control may sit under the device chrome at ${size}`).toEqual([]);
      const overflow = await page.evaluate(() => {
        const doc = document.documentElement;
        return { down: doc.scrollHeight - doc.clientHeight, across: doc.scrollWidth - doc.clientWidth };
      });
      expect(overflow, `V42: the blackjack table must fit ${size}`).toEqual({ down: 0, across: 0 });
      const clipped = await readClippedBoxes(page);
      expect(clipped, `V42: no blackjack box may clip its own content at ${size}`).toEqual([]);
    }
    /* oxlint-enable no-await-in-loop */
  });
}
