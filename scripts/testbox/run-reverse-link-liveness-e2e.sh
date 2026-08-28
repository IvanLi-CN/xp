#!/usr/bin/env bash
set -euo pipefail

XRAY_LOG="${TMPDIR:-/tmp}/xp-reverse-link-liveness-xray.log"
WEB_DIST_PLACEHOLDER='<!doctype html><title>xp reverse link liveness test</title>'
CREATED_WEB_DIST_PLACEHOLDER=false

if [[ ! -f web/dist/index.html ]]; then
  mkdir -p web/dist
  printf '%s\n' "$WEB_DIST_PLACEHOLDER" > web/dist/index.html
  CREATED_WEB_DIST_PLACEHOLDER=true
fi

xray run -c scripts/testbox/reverse-xray-spike-target.json >"$XRAY_LOG" 2>&1 &
XRAY_PID="$!"

cleanup() {
  local status=$?
  if [[ "$status" != 0 ]]; then
    cat "$XRAY_LOG" >&2
  fi
  kill "$XRAY_PID" 2>/dev/null || true
  wait "$XRAY_PID" 2>/dev/null || true
  rm -f "$XRAY_LOG"
  if [[ "$CREATED_WEB_DIST_PLACEHOLDER" == true ]] \
    && [[ "$(<web/dist/index.html)" == "$WEB_DIST_PLACEHOLDER" ]]; then
    rm -f web/dist/index.html
    rmdir web/dist 2>/dev/null || true
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

export XP_REVERSE_LIVENESS_XRAY_ADDR=127.0.0.1:10085
export XP_REVERSE_LIVENESS_XRAY_PID="$XRAY_PID"
cargo test --test reverse_link_liveness_e2e -- --ignored
