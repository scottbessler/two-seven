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
| e2e | `bun run test:e2e` (fast local loop; skips image comparison) |
| Run server | `cargo run` (:8080), or `./dev.sh` to restart on file change |
| Everything CI runs | `mise run check` |

## Notes

- `warnings = "deny"` and `clippy::all = "deny"` are set in `Cargo.toml`; a
  warning fails the build, so fix rather than `#[allow]`.
- Passkeys can't be driven headlessly. `PASSKEY_DISABLED=1` is set by `dev.sh`
  and by the Playwright web server; use it for any local run you need to sign
  into.
- e2e snapshots live beside their specs in `tests/e2e/*-snapshots/`. Baselines
  are rendered in the pinned Playwright container and are platform-independent;
  regenerate them with `bun run test:e2e:docker -- --update-snapshots`. The
  `chromium-mobile` project emulates an iPhone 15/16 Pro, safe-area insets
  included — don't relax the geometry or image tolerance to make a snapshot
  pass. `bun run test:e2e` remains the fast local loop and skips image
  comparison.
- `scripts/e2e-docker.sh` builds and runs the server on the host; only the
  browser and Playwright tests run inside the container.
- The UI font is Bitter, vendored as variable woff2 subsets in
  `public/vendor/bitter-v42-*.woff2` so an installed PWA keeps its type offline.
  The version lives in the filename because the `@font-face` src sits in static
  CSS that `asset()` never rewrites — to update the face, drop in new files
  under a new version and change `01-tokens.css`, `src/app.rs::asset_version`,
  and the preload in `src/render.rs` together. Card faces keep their own
  condensed stack (`--font-card`).
- `scripts/check_conservation.py <data-dir>` verifies the SPEC §V1/§V2/§V4 money
  invariants against a `DATA_PATH` tree.
