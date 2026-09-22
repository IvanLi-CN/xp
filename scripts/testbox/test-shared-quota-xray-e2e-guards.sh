#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNNER="$SCRIPT_DIR/run-shared-quota-xray-e2e.sh"
output_file="$(mktemp "${TMPDIR:-/tmp}/xp-testbox-guard.XXXXXX")"
trap 'rm -f "$output_file"' EXIT

expect_failure() {
  local expected="$1"
  shift
  if "$@" >"$output_file" 2>&1; then
    printf 'expected runner guard to fail: %s\n' "$expected" >&2
    exit 1
  fi
  grep -F "$expected" "$output_file"
}

expect_failure \
  'XP_TESTBOX_CARGO_CACHE_ROOT and XP_TESTBOX_CARGO_HOME are required' \
  env XP_RUN_MESH_RESOURCE=1 "$RUNNER"

expect_failure \
  'XP_TESTBOX_CACHE_SMOKE must be 0 or 1' \
  env \
    XP_RUN_MESH_RESOURCE=1 \
    XP_TESTBOX_CARGO_CACHE_ROOT=/srv/codex/agents/test/cache \
    XP_TESTBOX_CARGO_HOME=/srv/codex/caches/linux-amd64/cargo \
    XP_TESTBOX_CACHE_SMOKE=2 \
    "$RUNNER"

expect_failure \
  'the final Mesh resource gate requires XP_MESH_RESOURCE_DURATION_SECS=900' \
  env \
    XP_RUN_MESH_RESOURCE=1 \
    XP_TESTBOX_CARGO_CACHE_ROOT=/srv/codex/agents/test/cache \
    XP_TESTBOX_CARGO_HOME=/srv/codex/caches/linux-amd64/cargo \
    XP_MESH_RESOURCE_DURATION_SECS=600 \
    "$RUNNER"

expect_failure \
  'cache smoke requires XP_MESH_RESOURCE_DURATION_SECS=600' \
  env \
    XP_RUN_MESH_RESOURCE=1 \
    XP_TESTBOX_CARGO_CACHE_ROOT=/srv/codex/agents/test/cache \
    XP_TESTBOX_CARGO_HOME=/srv/codex/caches/linux-amd64/cargo \
    XP_TESTBOX_CACHE_SMOKE=1 \
    XP_MESH_RESOURCE_DURATION_SECS=900 \
    "$RUNNER"

printf 'shared testbox runner guard tests: passed\n'
