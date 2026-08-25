import type { Page } from "@playwright/test";

export type SafeAreaInsets = { top: number; right: number; bottom: number; left: number };
export type DeviceChrome = "dynamic-island" | "status-bar" | "landscape-island" | null;
export type EmulatedDevice = {
  viewport: { width: number; height: number };
  insets: SafeAreaInsets;
  chrome: DeviceChrome;
};

/**
 * The phone this app is actually played on: an iPhone 15/16 Pro (393x852pt,
 * Dynamic Island) installed to the home screen and launched as a standalone PWA.
 *
 * `apple-mobile-web-app-status-bar-style: black-translucent` puts the web view
 * behind the status bar, so the page owns all 852pt and the notch arrives as
 * `safe-area-inset-top: 59`. The home indicator overlays the bottom for another
 * 34pt. Landscape hides the status bar and moves the insets to the sides.
 *
 * Numbers follow the iOS safe-area table in
 * https://gist.github.com/fozzedout/5e77925381991a9570151550992baf14
 */
export const IPHONE_PORTRAIT: EmulatedDevice = {
  viewport: { width: 393, height: 852 },
  insets: { top: 59, right: 0, bottom: 34, left: 0 },
  chrome: "dynamic-island",
};

export const IPHONE_LANDSCAPE: EmulatedDevice = {
  viewport: { width: 852, height: 393 },
  insets: { top: 0, right: 59, bottom: 21, left: 59 },
  chrome: "landscape-island",
};

/** Largest Dynamic Island portrait: iPhone 15/16 Pro Max. */
export const IPHONE_MAX_PORTRAIT: EmulatedDevice = {
  viewport: { width: 430, height: 932 },
  insets: { top: 59, right: 0, bottom: 34, left: 0 },
  chrome: "dynamic-island",
};

/** Smallest supported iPhone: SE 3rd gen — home button, 20pt status bar, no indicator. */
export const IPHONE_SE_PORTRAIT: EmulatedDevice = {
  viewport: { width: 375, height: 667 },
  insets: { top: 20, right: 0, bottom: 0, left: 0 },
  chrome: "status-bar",
};

/**
 * Chromium reports every `env(safe-area-inset-*)` as 0, so the notch and home
 * indicator are invisible to a headless run. The split stylesheets funnel every inset
 * through `--safe-*` custom properties precisely so a test can pin them here.
 *
 * The same injection paints the device's own chrome — status bar, Dynamic
 * Island, home indicator — over the page. It is decoration for the snapshots, so
 * it never takes layout or pointer events; it exists so a reviewer can hold a
 * snapshot next to a real screenshot and compare like for like.
 */
function deviceScript(device: EmulatedDevice): string {
  const { insets, chrome } = device;
  const css = `:root{--safe-top:${insets.top}px;--safe-right:${insets.right}px;--safe-bottom:${insets.bottom}px;--safe-left:${insets.left}px}
.e2e-device-chrome{position:fixed;z-index:2147483647;pointer-events:none;color:#fff;font:600 15px/1 -apple-system,system-ui,sans-serif}
.e2e-device-chrome.status{inset:0 0 auto 0;display:flex;align-items:center;justify-content:space-between;height:${insets.top}px;padding:0 ${chrome === "dynamic-island" ? 30 : 14}px}
.e2e-device-chrome.status i{font-style:normal;letter-spacing:.06em}
.e2e-device-chrome.island{top:11px;left:50%;width:125px;height:37px;transform:translateX(-50%);border-radius:19px;background:#000}
.e2e-device-chrome.island-side{top:50%;left:11px;width:37px;height:125px;transform:translateY(-50%);border-radius:19px;background:#000}
.e2e-device-chrome.indicator{left:50%;bottom:8px;width:140px;height:5px;transform:translateX(-50%);border-radius:3px;background:#fff9}`;

  const parts: string[] = [];
  if (chrome === "dynamic-island" || chrome === "status-bar") {
    parts.push('<div class="e2e-device-chrome status"><i>9:41</i><i>&#x25AE;&#x25AE;&#x25AE; &#x25CF; 80</i></div>');
  }
  if (chrome === "dynamic-island") parts.push('<div class="e2e-device-chrome island"></div>');
  if (chrome === "landscape-island") parts.push('<div class="e2e-device-chrome island-side"></div>');
  if (insets.bottom >= 21) parts.push('<div class="e2e-device-chrome indicator"></div>');

  return `(() => {
    const apply = () => {
      for (const stale of document.querySelectorAll("style[data-device], .e2e-device-chrome")) stale.remove();
      document.documentElement.classList.add("standalone-pwa");
      const style = document.createElement("style");
      style.dataset.device = "";
      style.textContent = ${JSON.stringify(css)};
      document.head.append(style);
      document.body.insertAdjacentHTML("beforeend", ${JSON.stringify(parts.join(""))});
    };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", apply, { once: true });
    else apply();
  })()`;
}

/**
 * Pins the emulated device for every future navigation and for the document that
 * is already loaded. Repeat calls stack init scripts, but each injection drops
 * the previous overlay, so the newest device always wins.
 */
export async function applyDevice(page: Page, device: EmulatedDevice): Promise<void> {
  const script = deviceScript(device);
  await page.addInitScript({ content: script });
  await page.evaluate(script).catch(() => {
    // No document yet (fresh context) — the init script covers the first load.
  });
}

/** Switches the page to an emulated device: viewport, safe-area insets, and chrome. */
export async function useDevice(page: Page, device: EmulatedDevice): Promise<void> {
  await page.setViewportSize(device.viewport);
  await applyDevice(page, device);
}
