import { defineConfig, devices } from "@playwright/test";
import { IPHONE_PORTRAIT } from "./tests/e2e/devices";
import type { DeviceOptions } from "./tests/e2e/fixtures";

export default defineConfig<DeviceOptions>({
  testDir: "./tests/e2e",
  expect: {
    toHaveScreenshot: {
      animations: "disabled",
      maxDiffPixelRatio: 0.01,
    },
  },
  use: {
    baseURL: process.env.TEST_BASE_URL || "http://127.0.0.1:18080",
    trace: "on-first-retry",
    emulatedDevice: null,
  },
  webServer: {
    // The dialog needs a signed-in balance, and passkeys cannot be driven here.
    command: "PASSKEY_DISABLED=1 PORT=18080 cargo run",
    url: "http://127.0.0.1:18080/healthcheck",
    reuseExistingServer: true,
    timeout: 120_000,
  },
  projects: [
    {
      name: "chromium-desktop",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 1400 } },
    },
    {
      // Mirrors the phone the game is played on: an iPhone 15/16 Pro running the
      // installed PWA. Chromium still renders it, but the geometry — viewport and
      // safe-area insets both — is the iPhone's, so snapshots match the device.
      name: "chromium-mobile",
      use: {
        browserName: "chromium",
        viewport: IPHONE_PORTRAIT.viewport,
        emulatedDevice: IPHONE_PORTRAIT,
        deviceScaleFactor: 3,
        isMobile: true,
        hasTouch: true,
      },
    },
  ],
});
