import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
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
      name: "chromium-mobile",
      use: { ...devices["Pixel 7"], viewport: { width: 412, height: 915 } },
    },
  ],
});
