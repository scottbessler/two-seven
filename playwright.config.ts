import { defineConfig, devices } from "@playwright/test";
import { IPHONE_PORTRAIT } from "./tests/e2e/devices";
import type { DeviceOptions } from "./tests/e2e/fixtures";

/**
 * Image baselines are rendered by the pinned Playwright image, whose fonts and
 * rasterizer no other host reproduces byte for byte. Comparing them anywhere
 * else reports failures that mean nothing, so off the pinned image the default
 * is to skip image comparison outright rather than to compare and be ignored —
 * a run that silently proves nothing is worse than one that says so.
 * `bun run test:e2e:docker` sets E2E_IMAGES to turn them back on locally.
 */
const compareImages = Boolean(process.env.CI || process.env.E2E_IMAGES);

/** In CI the server is built by its own job; locally cargo still builds it. */
const serverCommand = process.env.E2E_SERVER_BIN
  ? `PASSKEY_DISABLED=1 PORT=18080 ${process.env.E2E_SERVER_BIN}`
  : "PASSKEY_DISABLED=1 PORT=18080 cargo run";

export default defineConfig<DeviceOptions>({
  testDir: "./tests/e2e",
  snapshotPathTemplate: "{testDir}/{testFileName}-snapshots/{arg}-{projectName}{ext}",
  ignoreSnapshots: !compareImages,
  expect: {
    toHaveScreenshot: {
      animations: "disabled",
      maxDiffPixelRatio: 0.01,
    },
  },
  reporter: [["html", { outputFolder: "playwright-report", open: "never" }]],
  use: {
    baseURL: process.env.TEST_BASE_URL || "http://127.0.0.1:18080",
    trace: "on-first-retry",
    emulatedDevice: null,
    launchOptions: {
      // GitHub's job containers give /dev/shm the default 64MB; a full-page
      // screenshot of the table outgrows it and Chromium dies. Not a rendering
      // flag — it only moves the backing store to /tmp.
      args: ["--disable-dev-shm-usage"],
    },
  },
  ...(process.env.TEST_BASE_URL
    ? {}
    : {
        webServer: {
          // The dialog needs a signed-in balance, and passkeys cannot be driven here.
          command: serverCommand,
          url: "http://127.0.0.1:18080/healthcheck",
          reuseExistingServer: true,
          timeout: 120_000,
        },
      }),
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
