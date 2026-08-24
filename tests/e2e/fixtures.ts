import { test as base, expect } from "@playwright/test";
import { applySafeArea, type SafeAreaInsets } from "./devices";

export type DeviceOptions = {
  /** Safe-area insets the project emulates; `null` leaves every inset at 0. */
  safeAreaInsets: SafeAreaInsets | null;
};

/**
 * The shared test object. It behaves exactly like Playwright's own except that a
 * project may declare `safeAreaInsets`, which are pinned on the page before the
 * test body runs so mobile layout is measured against a real iPhone's notch and
 * home indicator rather than Chromium's all-zero defaults.
 */
export const test = base.extend<DeviceOptions>({
  safeAreaInsets: [null, { option: true }],
  page: async ({ page, safeAreaInsets }, use) => {
    if (safeAreaInsets) await applySafeArea(page, safeAreaInsets);
    await use(page);
  },
});

export { expect };
