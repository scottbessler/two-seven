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
- e2e snapshots live beside their specs in `tests/e2e/*-snapshots/`. Baselines
  are rendered by the pinned Playwright image, which is the only environment
  that reproduces them: **`bun run test:e2e` skips image comparison entirely**
  unless `CI` or `E2E_IMAGES` is set, because a comparison against another
  host's fonts reports failures that mean nothing. Treat a green local run as
  saying nothing about pixels.
- To regenerate baselines, do any one of: comment `/update-snapshots` on the
  pull request, run the **Update snapshots** workflow (Actions → Update
  snapshots → Run workflow, pick the branch), or push a commit whose message
  contains `[update-snapshots]`. All three render them in the pinned image and
  commit them back to the branch, so no local Docker daemon is needed. (The
  comment and the message marker exist because `workflow_dispatch` only resolves
  once a workflow is on the default branch, which would leave a branch that
  changes rendering unable to update its own baselines. The marker counts
  anywhere in the push, not just on its last commit — a rendering change usually
  picks up review fixes on top of it before it goes up.)
  `bun run test:e2e:docker -- --update-snapshots` still works if you have one.
  The regeneration commit is pushed with `GITHUB_TOKEN`, and GitHub does not
  start workflows for those pushes — re-run CI by hand (Actions → CI → Run
  workflow) or push again to verify the new baselines.
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
  review and is stable on every platform; images are for what only pixels catch
  (shadow, radius, gradient, stacking).
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
- `scripts/check_conservation.py <data-dir>` verifies the SPEC §V1/§V2/§V4 money
  invariants against a `DATA_PATH` tree.
