import { expect, type Page } from "@playwright/test";

/**
 * A geometry snapshot: box and chosen computed styles per selector, as JSON.
 *
 * This exists because an image baseline is a poor regression *report*. When the
 * seat stack silently dropped from 12.8px to 11px, every element on the page
 * shifted and the PNG diff was a solid wall of red — it proved something broke
 * without saying what, and the cause took a bisect to find. The same regression
 * as a layout snapshot is two lines in the PR diff:
 *
 *     -  ".seat-stack": { "fontSize": "12.8px", "color": "rgb(217, 173, 85)" }
 *     +  ".seat-stack": { "fontSize": "11px",   "color": "rgb(169, 194, 183)" }
 *
 * These also survive a host change that images cannot: text metrics still move
 * boxes, but rounding to whole pixels absorbs the sub-pixel differences that
 * make PNGs platform-specific.
 *
 * Use this for layout and applied style. Keep images for what only pixels catch:
 * shadow, radius, gradient, stacking order.
 */

/** Styles worth recording. Anything that moves a box or recolours it. */
const TRACKED = [
  "display",
  "position",
  "fontSize",
  "fontWeight",
  "color",
  "backgroundColor",
  "borderRadius",
  "gridTemplateColumns",
  "flexDirection",
] as const;

type Box = { x: number; y: number; width: number; height: number };
type Recorded = Box & Partial<Record<(typeof TRACKED)[number], string>>;
export type LayoutSnapshot = Record<string, Recorded | Recorded[] | "absent">;

/**
 * Records `selectors` against the live page. A selector matching several nodes
 * records an array, so a seat row is one entry rather than six near-duplicates.
 */
export async function readLayout(page: Page, selectors: string[]): Promise<LayoutSnapshot> {
  return page.evaluate(
    ({ wanted, tracked }: { wanted: string[]; tracked: readonly string[] }) => {
      const record = (element: Element) => {
        const box = element.getBoundingClientRect();
        const styles = getComputedStyle(element);
        // Whole pixels: absorbs the sub-pixel drift that makes images host-specific.
        const entry: Record<string, number | string> = {
          x: Math.round(box.x),
          y: Math.round(box.y),
          width: Math.round(box.width),
          height: Math.round(box.height),
        };
        for (const property of tracked) {
          const value = styles[property as keyof CSSStyleDeclaration] as string;
          // Resolved track sizes come back sub-pixel (`284.453px`), which is the
          // one part of a computed style that drifts with text metrics. Round it
          // for the same reason the boxes are rounded.
          entry[property] = value.replaceAll(/(\d+\.\d+)px/g, (_, px: string) => `${Math.round(Number(px))}px`);
        }
        return entry;
      };
      const snapshot: Record<string, unknown> = {};
      for (const selector of wanted) {
        const matches = [...document.querySelectorAll(selector)];
        if (matches.length === 0) snapshot[selector] = "absent";
        else if (matches.length === 1) snapshot[selector] = record(matches[0]);
        else snapshot[selector] = matches.map(record);
      }
      return snapshot;
    },
    { wanted: selectors, tracked: TRACKED as readonly string[] },
  );
}

/**
 * Records `selectors` and compares them with the stored JSON baseline.
 *
 * Baselines are plain text and land in `*-snapshots/` beside the specs, so they
 * regenerate with `--update-snapshots` like any other snapshot — but unlike the
 * images they are reviewable, and they compare on every platform, so they are
 * checked even when `bun run test:e2e` is skipping image comparison.
 */
export async function expectLayout(page: Page, name: string, selectors: string[]): Promise<void> {
  const snapshot = await readLayout(page, selectors);
  expect(JSON.stringify(snapshot, null, 2)).toMatchSnapshot(`${name}.json`);
}
