# Working in this repo

Rust (axum) backend in `src/`, vanilla-JS PWA frontend in `public/`, Playwright
e2e in `tests/e2e/`. `SPEC.md` is the source of truth for game rules;
`STATECHART.md` describes the table state machine.

## Setup

```sh
./scripts/setup.sh
```

Idempotent: enables the repo git hooks, installs JS deps, resolves a Chromium
for Playwright, and warms the Rust build. `SKIP_BUILD=1` skips the slow build,
`SKIP_BROWSERS=1` skips browser setup. Claude Code web sessions run this
automatically via `.claude/hooks/session-start.sh`.

Toolchain: Rust 1.90+ (edition 2024), bun 1.3.13, node 22 — see `.mise.toml`.

## Commands

| Task | Command |
| --- | --- |
| Rust tests | `cargo test --locked` |
| Rust lint | `cargo fmt --check && cargo clippy --locked --all-targets --all-features` |
| JS lint | `bun run lint` (`bun run lint:fix` to autofix) |
| e2e | `bun run test:e2e` (fast local loop; skips image comparison — see Notes) |
| Run server | `cargo run` (:8080), or `./dev.sh` to restart on file change |
| Everything CI runs | `mise run check` |

## Notes

- `warnings = "deny"` and `clippy::all = "deny"` are set in `Cargo.toml`; a
  warning fails the build, so fix rather than `#[allow]`.
- Passkeys can't be driven headlessly. `PASSKEY_DISABLED=1` is set by `dev.sh`
  and by the Playwright web server; use it for any local run you need to sign
  into.
- e2e snapshots live beside their specs: reviewable geometry baselines under
  `tests/e2e/*-snapshots/json/`, image baselines under
  `tests/e2e/*-snapshots/images/`. Images are rendered by the pinned Playwright
  environment: **`bun run test:e2e` skips image comparison entirely**
  unless `CI` or `E2E_IMAGES` is set, because a comparison against another
  host's fonts reports failures that mean nothing. Treat a green local run as
  saying nothing about pixels.
- Every same-repository pull request automatically regenerates image baselines
  in the pinned environment and commits image-only changes back to its branch;
  JSON geometry changes stay explicit and reviewable. The workflow dispatches
  CI after its bot commit. Manual fallbacks remain: comment `/update-snapshots`,
  run **Update image snapshots** (pick the branch), or use
  `bun run test:e2e:docker -- --update-snapshots` if you have Docker.
- On Linux you can compare images without any container: the pinned fonts and
  rasterizer flags make a plain checkout match CI byte for byte, verified across
  a different Chromium build. `E2E_IMAGES=1 bun run test:e2e` opts in. macOS
  still will not match — CoreText and FreeType never agree — so that stays a
  container or CI job.
- The `chromium-mobile` project emulates an iPhone 15/16 Pro, safe-area insets
  included — don't relax the geometry or image tolerance to make a snapshot
  pass.
- Prefer a geometry snapshot to an image one. `expectLayout` in
  `tests/e2e/layout.ts` records boxes and computed styles as JSON that diffs in
  review; images are for what only pixels catch (shadow, radius, gradient,
  stacking). **A layout baseline is not host-independent either**: sizes and
  styles hold everywhere, but a box's absolute `y` moves with the text metrics
  of everything above it — a heading and a paragraph rendered off-container put
  a whole page ten pixels down. So regenerate JSON baselines in the container
  like images, and never run `--update-snapshots` over a spec you are not
  regenerating: it silently rewrites correct baselines with this host's numbers,
  and the damage only shows up in CI. `git diff` on a `*-snapshots/json/` file
  you did not mean to touch is the tell.
- CI runs the e2e job *inside* the Playwright image and takes the server binary
  from the `server` job, so no Docker daemon is involved.
  `scripts/e2e-docker.sh` is the local-only path: it runs the server on the
  host and the browser in the container.
- The UI font is Bitter, vendored as variable woff2 subsets in
  `public/vendor/bitter-v42-*.woff2` so an installed PWA keeps its type offline.
  The version lives in the filename because the `@font-face` src sits in static
  CSS that `asset()` never rewrites — to update the face, drop in new files
  under a new version and change `01-tokens.css`, `src/app.rs::asset_version`,
  and the preload in `src/render.rs` together. Card faces share `--font-ui`;
  only their glyph size is derived from the card's own width, which is why
  `04-cards.css` is the one file exempt from the type scale.
- Game state is moving from the JSON tree under `DATA_PATH` to SQLite in
  `DATA_PATH/two-seven.db` — see `STORAGE.md` for why, and for which stores
  have crossed over. Schema changes are appended to `MIGRATIONS` in
  `src/db.rs`; a migration that has shipped is never edited.
- `scripts/check_conservation.py <data-dir>` verifies the SPEC §V1/§V2/§V4 money
  invariants against a `DATA_PATH` tree.
- Roulette is at `/roulette` (the game) and `/roulette-test` (the wheel on its
  own, with every motion number as a slider). `src/roulette.rs` derives all 157
  legal bets from the board's geometry and is the only thing that prices one —
  the page is served that same catalogue, so odds and coverage have a single
  source and `roulette-board.js` only decides *which* id a touch names. The
  e2e walks every zone of every square against the catalogue, which is what
  keeps the two boards one board. Chips on the felt are a claim on the stack
  rather than a withdrawal, so a table is always worth exactly `stack`.
- The wheel itself is a motion study first: `roulette-spin.js` is pure
  and simulates the ball honestly, then turns the *rotor* so the pocket it
  happened to land in carries the number that was asked for -- fret geometry
  repeats every pocket, so that correction is a whole number of pockets and the
  simulated path stays valid. Nothing about the ball's flight depends on the
  outcome, which is what makes the bouncing worth watching. A rotor that is
  already turning cannot jump to the phase an outcome needs, so `rotorAt` walks
  it there while the ball is still on the track. `roulette-wheel.js` only reads
  the plan's timeline, so motion is identical at any frame rate.
