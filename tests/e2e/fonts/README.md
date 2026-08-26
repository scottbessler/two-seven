# Pinned test fonts

These are **test-harness assets only** — the app never serves them and
`public/css/` never references them.

The app self-hosts Bitter (`public/vendor/bitter-v42-*.woff2`) and uses it for
every piece of text, labels and card faces alike, so it is already deterministic
and the harness renders it rather than substituting anything. These two subsets
cover only what Bitter does not: `tests/e2e/rendering.ts` appends them as
fallbacks so those glyphs cannot reach a host font.

| File | Family | Covers | Licence |
| --- | --- | --- | --- |
| `symbols2.ttf` | Noto Sans Symbols 2 (subset) | `♠ ♥ ♦ ♣ ⚙ ⓘ` | OFL-1.1 |
| `emoji.ttf` | Noto Emoji (subset, monochrome) | `🪙` | OFL-1.1 |

Both are cut to exactly the code points the app renders, which is why they are a
few KB each. If a new non-ASCII glyph is added to the UI, extend `PINNED_GLYPHS`
and the subset rather than letting it fall back to a system font — the
`pins every non-ASCII glyph to a vendored font` spec fails when a glyph has no
coverage.
