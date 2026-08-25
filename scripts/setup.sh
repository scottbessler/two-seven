#!/usr/bin/env bash
# Prepare a working two-seven dev environment: git hooks, JS deps, browsers, Rust build.
# Safe to re-run. Used by humans, by `mise run setup`, and by the Claude SessionStart hook.
#
#   ./scripts/setup.sh              full setup
#   SKIP_BUILD=1 ./scripts/setup.sh skip the (slow) Rust warm build
#   SKIP_BROWSERS=1 ./scripts/setup.sh  skip Playwright browser setup
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || dirname "$(dirname "$0")")"

log() { printf '[setup] %s\n' "$*"; }
die() { printf '[setup] error: %s\n' "$*" >&2; exit 1; }

# --- toolchain ---------------------------------------------------------------
command -v cargo >/dev/null 2>&1 || die "cargo not found. Install Rust via https://rustup.rs (edition 2024 needs 1.85+)."
log "cargo $(cargo --version | awk '{print $2}'), rustc $(rustc --version | awk '{print $2}')"

for component in fmt clippy; do
  cargo "$component" --version >/dev/null 2>&1 ||
    die "cargo $component missing. Run: rustup component add $([ "$component" = fmt ] && echo rustfmt || echo clippy)"
done

if command -v bun >/dev/null 2>&1; then
  JS_RUN="bun run"
  JS_INSTALL="bun install"
  log "bun $(bun --version)"
elif command -v npm >/dev/null 2>&1; then
  JS_RUN="npm run"
  JS_INSTALL="npm install"
  log "bun not found; falling back to npm $(npm --version)"
else
  die "neither bun nor npm found. Install bun 1.3.13 (see .mise.toml)."
fi

# --- git hooks ---------------------------------------------------------------
# .githooks/pre-commit runs fmt+clippy+oxlint; pre-push runs the full gate on main.
if git rev-parse --git-dir >/dev/null 2>&1; then
  git config core.hooksPath .githooks
  log "git hooks -> .githooks"
fi

# --- javascript deps ---------------------------------------------------------
log "installing JS deps ($JS_INSTALL)"
$JS_INSTALL

# --- playwright browsers -----------------------------------------------------
# Managed environments (Claude Code on the web) ship a prebuilt Chromium under
# PLAYWRIGHT_BROWSERS_PATH, but its revision tracks the *preinstalled* Playwright
# rather than the version this repo pins. Link the revision our Playwright looks
# for at the one that is actually on disk instead of downloading a second copy.
setup_browsers() {
  local browsers_dir="${PLAYWRIGHT_BROWSERS_PATH:-}"
  local want
  want="$(node -e 'try{console.log(require("playwright-core").chromium.executablePath())}catch(e){}' 2>/dev/null || true)"
  [ -n "$want" ] || { log "playwright-core not resolvable; skipping browser setup"; return 0; }

  if [ -x "$want" ]; then
    log "chromium ready: $want"
    return 0
  fi

  local want_rev
  want_rev="$(printf '%s' "$want" | sed -n 's|.*/chromium-\([0-9][0-9]*\)/.*|\1|p')"

  if [ -n "$browsers_dir" ] && [ -n "$want_rev" ] && [ -d "$browsers_dir" ] && [ -w "$browsers_dir" ]; then
    local have_rev=""
    for dir in "$browsers_dir"/chromium-[0-9]*; do
      [ -d "$dir" ] || continue
      have_rev="$(basename "$dir" | sed -n 's|chromium-\([0-9][0-9]*\)|\1|p')"
    done
    if [ -n "$have_rev" ]; then
      ln -sfn "$browsers_dir/chromium-$have_rev" "$browsers_dir/chromium-$want_rev"
      # Headless runs use the separate headless_shell build, so link it too.
      [ -d "$browsers_dir/chromium_headless_shell-$have_rev" ] &&
        ln -sfn "$browsers_dir/chromium_headless_shell-$have_rev" "$browsers_dir/chromium_headless_shell-$want_rev"
      log "linked chromium-$want_rev -> chromium-$have_rev (preinstalled)"
      return 0
    fi
  fi

  log "downloading chromium via playwright"
  if command -v bunx >/dev/null 2>&1; then bunx playwright install chromium; else npx playwright install chromium; fi
}

if [ "${SKIP_BROWSERS:-0}" = "1" ]; then
  log "SKIP_BROWSERS=1; skipping browser setup"
else
  setup_browsers
fi

# --- rust build --------------------------------------------------------------
log "fetching crates"
cargo fetch --locked

if [ "${SKIP_BUILD:-0}" = "1" ]; then
  log "SKIP_BUILD=1; skipping warm build"
else
  # Builds lib, bins and test targets so the first `cargo test` / e2e run is fast.
  log "building (debug, with test targets) — first run takes a few minutes"
  cargo build --locked --tests
fi

cat <<'EOM'

[setup] done. Common commands:
  cargo test --locked            Rust unit + integration tests
  cargo run                      serve on :8080 (PASSKEY_DISABLED=1 to skip passkeys)
  ./dev.sh                       cargo run with file-watch restart
  cargo fmt && cargo clippy --all-targets
  bun run lint                   oxlint over public/ and tests/
  bun run test:e2e               Playwright e2e (starts its own server on :18080)
  mise run check                 everything CI runs, if you have mise
EOM
