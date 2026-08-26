import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, type Locator, type Page } from "@playwright/test";

// Playwright transpiles the suite to CJS (package.json declares no module type),
// so `__dirname` resolves here and `import.meta.url` would not.
// eslint-disable-next-line no-underscore-dangle
declare const __dirname: string;

/**
 * Pins everything about text rendering that the host would otherwise decide.
 *
 * Image baselines used to be reproducible only inside the Playwright container,
 * because `system-ui` resolves to SF Pro on macOS and to whatever fontconfig
 * picks on Linux, and `"Arial Narrow"` — the first card face in `04-cards.css` —
 * exists on neither CI nor most Linux boxes, so the card ranks were already
 * rendering in an unpredictable fallback. Injecting a vendored font set removes
 * that variable, which is what lets a plain Linux checkout match CI.
 *
 * This is deliberately harness-only. Production keeps its native stacks so the
 * iPhone the game is played on still gets SF Pro; `frontend_assets.rs` pins
 * those stacks so a change to them is a deliberate edit rather than a drift.
 */
const fontsDir = join(__dirname, "fonts");

function face(family: string, file: string, format: string, weight: string): string {
  const mime = format === "woff2" ? "font/woff2" : "font/ttf";
  const data = readFileSync(join(fontsDir, file)).toString("base64");
  return `@font-face{font-family:"${family}";src:url(data:${mime};base64,${data}) format("${format}");font-weight:${weight};font-style:normal;font-display:block}`;
}

/**
 * Glyphs the UI renders outside ASCII, and the pinned family that must cover
 * each. `fontsCoverEveryGlyph` asserts the page never falls back past these.
 */
export const PINNED_GLYPHS = "·—×…♠♥♦♣⚙ⓘ🪙";

const FONT_CSS = [
  face("E2E Sans", "roboto.woff2", "woff2", "100 900"),
  face("E2E Condensed", "roboto-condensed.woff2", "woff2", "100 900"),
  face("E2E Symbols", "symbols2.ttf", "truetype", "400"),
  face("E2E Emoji", "emoji.ttf", "truetype", "400"),
].join("");

/**
 * Same selectors and specificity the app uses, appended after its stylesheets,
 * so the cascade settles this on source order alone — no `!important` needed.
 */
const PIN_CSS = `body,button,input,select,textarea{font-family:"E2E Sans","E2E Symbols","E2E Emoji",sans-serif}
.playing-card{font-family:"E2E Condensed","E2E Symbols",sans-serif}`;

function pinScript(): string {
  const css = FONT_CSS + PIN_CSS;
  return `(() => {
    const apply = () => {
      if (document.querySelector("style[data-e2e-fonts]")) return;
      const style = document.createElement("style");
      style.dataset.e2eFonts = "";
      style.textContent = ${JSON.stringify(css)};
      document.head.append(style);
    };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", apply, { once: true });
    else apply();
  })()`;
}

/** Pins the font set for every future navigation and the current document. */
export async function pinRendering(page: Page): Promise<void> {
  const script = pinScript();
  await page.addInitScript({ content: script });
  await page.evaluate(script).catch(() => {
    // No document yet on a fresh context — the init script covers the first load.
  });
}

/**
 * Reports any glyph in `PINNED_GLYPHS` the pinned families do not cover, which
 * would silently fall through to a host font and desynchronise the baselines.
 */
export async function uncoveredGlyphs(page: Page): Promise<string[]> {
  return page.evaluate((glyphs: string) => {
    const families = ['"E2E Sans", "E2E Symbols", "E2E Emoji"', '"E2E Condensed", "E2E Symbols"'];
    return [...glyphs].filter((glyph) =>
      families.every((family) => !document.fonts.check(`16px ${family}`, glyph)),
    );
  }, PINNED_GLYPHS);
}

/**
 * Whether image baselines are meaningful in this environment.
 *
 * Only the pinned Playwright image reproduces them; anywhere else a comparison
 * reports differences that say nothing about the change under test. This is
 * checked at the call site rather than through Playwright's `ignoreSnapshots`,
 * because that switch also disables `toMatchSnapshot` — and the layout
 * snapshots in `layout.ts` are platform-independent by construction and should
 * run everywhere, including on a laptop with no container.
 */
export const COMPARE_IMAGES = Boolean(process.env.CI || process.env.E2E_IMAGES);

/** Image comparison, skipped outside the environment that can reproduce it. */
export async function expectImage(
  target: Page | Locator,
  name: string,
  options?: { fullPage?: boolean },
): Promise<void> {
  if (!COMPARE_IMAGES) return;
  await expect(target).toHaveScreenshot(name, options);
}
