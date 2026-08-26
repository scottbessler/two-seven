import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, type Locator, type Page } from "@playwright/test";

// Playwright transpiles the suite to CJS (package.json declares no module type),
// so `__dirname` resolves here and `import.meta.url` would not.
// eslint-disable-next-line no-underscore-dangle
declare const __dirname: string;

/**
 * Pins the glyphs production's own font cannot cover, so image baselines do not
 * depend on what the host happens to have installed.
 *
 * The app self-hosts Bitter and uses it for every piece of text — labels and
 * card faces alike — which is already deterministic, so the harness
 * deliberately does *not* substitute it: snapshots show the typeface real users
 * see. What Bitter does not carry is the symbol set (`♠ ♥ ♦ ♣ ⚙ ⓘ`) and the
 * coin emoji, which would otherwise fall through to a host font. Those are
 * vendored as subsets and appended as explicit fallbacks.
 *
 * `production_font_stacks_are_deliberate` pins `--font-ui`, so changing the
 * app's face fails that test and sends you here to keep the two in step.
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
  face("E2E Symbols", "symbols2.ttf", "truetype", "400"),
  face("E2E Emoji", "emoji.ttf", "truetype", "400"),
].join("");

/**
 * Production's own primary, then the vendored fallbacks. Same selectors and
 * specificity the app uses, appended after its stylesheets, so the cascade
 * settles this on source order alone — no `!important` needed.
 */
const PIN_FAMILIES = `"Bitter","E2E Symbols","E2E Emoji",serif`;
const PIN_CSS = `body,button,input,select,textarea,.playing-card{font-family:${PIN_FAMILIES}}`;

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
  return page.evaluate(
    ({ glyphs, families }: { glyphs: string; families: string }) =>
      [...glyphs].filter((glyph) => !document.fonts.check(`16px ${families}`, glyph)),
    { glyphs: PINNED_GLYPHS, families: PIN_FAMILIES },
  );
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
