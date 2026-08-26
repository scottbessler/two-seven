# Pinned test fonts

These are **test-harness assets only** — the app never serves them and
`public/css/` never references them. `tests/e2e/rendering.ts` injects them into
every page under test so image baselines do not depend on which fonts the host
happens to have installed.

Production keeps its native stacks on purpose: `system-ui` gives the target
iPhone its own SF Pro, which is the right typeface for the device.

| File | Family | Covers | Licence |
| --- | --- | --- | --- |
| `roboto.woff2` | Roboto (variable 100–900) | UI and body text | Apache-2.0 |
| `roboto-condensed.woff2` | Roboto Condensed (variable 100–900) | card ranks | Apache-2.0 |
| `symbols2.ttf` | Noto Sans Symbols 2 (subset) | `♠ ♥ ♦ ♣ ⚙ ⓘ` | OFL-1.1 |
| `emoji.ttf` | Noto Emoji (subset, monochrome) | `🪙` | OFL-1.1 |

The two subsets are cut to exactly the code points the app renders, which is why
they are a few KB each. If a new non-ASCII glyph is added to the UI, extend the
subset rather than letting it fall back to a system font — `fontsCoverEveryGlyph`
in `tests/e2e/rendering.ts` fails the suite when a glyph has no pinned coverage.
