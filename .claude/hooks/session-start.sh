#!/usr/bin/env bash
# SessionStart hook for Claude Code on the web: makes tests and linters runnable
# before the agent starts. Local sessions are left alone — run scripts/setup.sh
# yourself there.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$(dirname "$(dirname "$0")")")}"

./scripts/setup.sh

# Persist for the session so Playwright and the dev server behave in later shells.
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
  [ -n "${PLAYWRIGHT_BROWSERS_PATH:-}" ] &&
    echo "export PLAYWRIGHT_BROWSERS_PATH=\"$PLAYWRIGHT_BROWSERS_PATH\"" >> "$CLAUDE_ENV_FILE"
  # Passkey registration cannot be driven headlessly; every non-prod entrypoint
  # in this repo already assumes this.
  echo 'export PASSKEY_DISABLED=1' >> "$CLAUDE_ENV_FILE"
fi
