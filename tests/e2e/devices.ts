import type { Page } from "@playwright/test";

export type SafeAreaInsets = { top: number; right: number; bottom: number; left: number };
export type EmulatedDevice = { viewport: { width: number; height: number }; insets: SafeAreaInsets };

/**
 * The phone this app is actually played on: an iPhone 15/16 Pro (393x852pt,
 * Dynamic Island) installed to the home screen and launched as a standalone PWA.
 *
 * Standalone mode plus `apple-mobile-web-app-status-bar-style: default` reserves
 * the 59pt status bar above the web view, so the page only gets 852 - 59 = 793pt
 * and reads `safe-area-inset-top` as 0. The home indicator still overlays the web
 * view, so the bottom inset stays 34pt. Landscape hides the status bar and moves
 * the insets to the sides (59pt) with a 21pt indicator strip along the bottom.
 *
 * Numbers follow the iOS safe-area table in
 * https://gist.github.com/fozzedout/5e77925381991a9570151550992baf14
 */
export const IPHONE_PORTRAIT: EmulatedDevice = {
  viewport: { width: 393, height: 793 },
  insets: { top: 0, right: 0, bottom: 34, left: 0 },
};

export const IPHONE_LANDSCAPE: EmulatedDevice = {
  viewport: { width: 852, height: 393 },
  insets: { top: 0, right: 59, bottom: 21, left: 59 },
};

/** Largest Dynamic Island portrait: iPhone 15/16 Pro Max, 932 - 59 status bar. */
export const IPHONE_MAX_PORTRAIT: EmulatedDevice = {
  viewport: { width: 430, height: 873 },
  insets: { top: 0, right: 0, bottom: 34, left: 0 },
};

/** Smallest supported iPhone: SE 3rd gen, 667 - 20 status bar, no insets at all. */
export const IPHONE_SE_PORTRAIT: EmulatedDevice = {
  viewport: { width: 375, height: 647 },
  insets: { top: 0, right: 0, bottom: 0, left: 0 },
};

/**
 * Chromium reports every `env(safe-area-inset-*)` as 0, so the notch and home
 * indicator are invisible to a headless run. `app.css` funnels every inset
 * through `--safe-*` custom properties precisely so a test can pin them here.
 */
function insetStyleScript(insets: SafeAreaInsets): string {
  const css = `:root{--safe-top:${insets.top}px;--safe-right:${insets.right}px;--safe-bottom:${insets.bottom}px;--safe-left:${insets.left}px}`;
  return `(() => {
    const apply = () => {
      for (const stale of document.querySelectorAll("style[data-safe-area]")) stale.remove();
      const style = document.createElement("style");
      style.dataset.safeArea = "";
      style.textContent = ${JSON.stringify(css)};
      document.head.append(style);
    };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", apply, { once: true });
    else apply();
  })()`;
}

/**
 * Pins the emulated insets for every future navigation and for the document that
 * is already loaded. Repeat calls stack init scripts, but each injection drops
 * the previous `[data-safe-area]` style, so the newest values always win.
 */
export async function applySafeArea(page: Page, insets: SafeAreaInsets): Promise<void> {
  const script = insetStyleScript(insets);
  await page.addInitScript({ content: script });
  await page.evaluate(script).catch(() => {
    // No document yet (fresh context) — the init script covers the first load.
  });
}

/** Switches the page to an emulated device: viewport and safe-area insets together. */
export async function useDevice(page: Page, device: EmulatedDevice): Promise<void> {
  await page.setViewportSize(device.viewport);
  await applySafeArea(page, device.insets);
}
