import { test as base, expect } from "@playwright/test";
import { applyDevice, type EmulatedDevice } from "./devices";

export type DeviceOptions = {
  /** Phone this project emulates; `null` runs plain desktop Chromium. */
  emulatedDevice: EmulatedDevice | null;
};

/**
 * The shared test object. It behaves exactly like Playwright's own except that a
 * project may declare `emulatedDevice`, whose safe-area insets and device chrome
 * are pinned on the page before the test body runs, so mobile layout is measured
 * against a real iPhone's notch and home indicator rather than Chromium's
 * all-zero defaults.
 */
export const test = base.extend<DeviceOptions>({
  emulatedDevice: [null, { option: true }],
  page: async ({ page, emulatedDevice }, use) => {
    if (emulatedDevice) await applyDevice(page, emulatedDevice);
    await use(page);
  },
});

export { expect };
