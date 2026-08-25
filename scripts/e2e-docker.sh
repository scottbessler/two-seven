#!/usr/bin/env bash
set -Eeuo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

image="mcr.microsoft.com/playwright:v1.54.1-noble"
expected_version="$(node -p 'require("./package.json").devDependencies["@playwright/test"]')"
if [[ "$expected_version" != "1.54.1" ]]; then
  printf 'Expected @playwright/test 1.54.1, found %s\n' "$expected_version" >&2
  exit 1
fi

if ! docker run --rm --volume "$repo_root:/work" --workdir /work "$image" \
  test -x node_modules/.bin/playwright; then
  printf '%s is not visible inside the container.\n' "$repo_root" >&2
  printf 'Add it under Docker Desktop > Settings > Resources > File sharing, or run bun install.\n' >&2
  exit 1
fi

cargo build --locked

server_log="${TMPDIR:-/tmp}/two-seven-e2e-server.$$"
server_pid=""
cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -f "$server_log"
}
trap cleanup EXIT INT TERM

PASSKEY_DISABLED=1 PORT=18080 target/debug/two-seven >"$server_log" 2>&1 &
server_pid=$!
for _ in $(seq 1 120); do
  if curl --fail --silent --show-error http://127.0.0.1:18080/healthcheck >/dev/null; then
    break
  fi
  if ! kill -0 "$server_pid" 2>/dev/null; then
    cat "$server_log" >&2
    exit 1
  fi
  sleep 1
done
if ! curl --fail --silent --show-error http://127.0.0.1:18080/healthcheck >/dev/null; then
  cat "$server_log" >&2
  exit 1
fi

docker run --rm --ipc=host \
  --init \
  --user "$(id -u):$(id -g)" \
  --add-host=host.docker.internal:host-gateway \
  --env HOME=/tmp/playwright-home \
  --env TEST_BASE_URL=http://host.docker.internal:18080 \
  --volume "$repo_root:/work" \
  --workdir /work \
  "$image" \
  node_modules/.bin/playwright test "$@"
