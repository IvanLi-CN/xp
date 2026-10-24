#!/usr/bin/env bash

: "${TESTBOX:?set TESTBOX to an authorized SSH target}"
: "${XP_TESTBOX_REMOTE_ROOT:?set XP_TESTBOX_REMOTE_ROOT to a writable remote root}"

case "${USER:-}" in
  ""|*[!A-Za-z0-9._-]*)
    echo "USER must contain only safe path characters" >&2
    exit 2
    ;;
esac

TESTBOX_REMOTE_ROOT="${XP_TESTBOX_REMOTE_ROOT%/}"
case "$TESTBOX_REMOTE_ROOT" in
  ""|/|*/..|*/../*)
    echo "XP_TESTBOX_REMOTE_ROOT must be a dedicated absolute directory" >&2
    exit 2
    ;;
  *[!A-Za-z0-9._/-]*)
    echo "XP_TESTBOX_REMOTE_ROOT must contain only safe path characters" >&2
    exit 2
    ;;
  /*) ;;
  *)
    echo "XP_TESTBOX_REMOTE_ROOT must be absolute" >&2
    exit 2
    ;;
esac

testbox_validate_component() {
  local label="$1"
  local value="$2"
  case "$value" in
    ""|*[!A-Za-z0-9._-]*)
      echo "$label must contain only safe path characters" >&2
      exit 2
      ;;
  esac
}

testbox_remote_root_preflight() {
  ssh -o BatchMode=yes "$TESTBOX" \
    "set -euo pipefail; root='$TESTBOX_REMOTE_ROOT'; mkdir -p -- \"\$root\"; test -d \"\$root\"; test ! -L \"\$root\"; test \"\$(readlink -f -- \"\$root\")\" = \"\$root\""
}

testbox_remote_run_guard() {
  local remote_run="$1"
  local root_b64 remote_run_b64
  root_b64="$(printf '%s' "$TESTBOX_REMOTE_ROOT" | base64 | tr -d '\n')"
  remote_run_b64="$(printf '%s' "$remote_run" | base64 | tr -d '\n')"
  ssh -o BatchMode=yes "$TESTBOX" \
    "TESTBOX_REMOTE_ROOT_B64='$root_b64' REMOTE_RUN_B64='$remote_run_b64' bash -s" <<'REMOTE_RUN_GUARD'
set -euo pipefail
TESTBOX_REMOTE_ROOT="$(printf '%s' "${TESTBOX_REMOTE_ROOT_B64:?}" | base64 -d)"
REMOTE_RUN="$(printf '%s' "${REMOTE_RUN_B64:?}" | base64 -d)"
assert_no_symlink_path() {
  local path="$1"
  while [ "$path" != "/" ] && [ "$path" != "." ]; do
    if [ -L "$path" ]; then
      return 2
    fi
    path="$(dirname "$path")"
  done
}
test ! -L "$TESTBOX_REMOTE_ROOT"
test "$(readlink -f -- "$TESTBOX_REMOTE_ROOT")" = "$TESTBOX_REMOTE_ROOT"
assert_no_symlink_path "$TESTBOX_REMOTE_ROOT"
test ! -L "$REMOTE_RUN"
assert_existing_no_symlink_path() {
  local path="$1"
  while [ ! -e "$path" ] && [ ! -L "$path" ] && [ "$path" != "/" ]; do
    path="$(dirname "$path")"
  done
  assert_no_symlink_path "$path"
}
if ! assert_existing_no_symlink_path "$REMOTE_RUN"; then
  exit 2
fi
remote_run_real="$(readlink -m -- "$REMOTE_RUN")"
case "$remote_run_real" in
  "$TESTBOX_REMOTE_ROOT"/agents/*/runs/*|"$TESTBOX_REMOTE_ROOT"/workspaces/*/runs/*) ;;
  *) exit 2 ;;
esac
assert_no_symlink_path "$remote_run_real"
REMOTE_RUN="$remote_run_real"
printf '%s\n' "$REMOTE_RUN"
REMOTE_RUN_GUARD
}

testbox_remote_cleanup() {
  local remote_run="$1"
  local run_id="${2:-}"
  local root_b64 remote_run_b64 run_id_b64
  root_b64="$(printf '%s' "$TESTBOX_REMOTE_ROOT" | base64 | tr -d '\n')"
  remote_run_b64="$(printf '%s' "$remote_run" | base64 | tr -d '\n')"
  run_id_b64="$(printf '%s' "$run_id" | base64 | tr -d '\n')"
  ssh -o BatchMode=yes "$TESTBOX" \
    "TESTBOX_REMOTE_ROOT_B64='$root_b64' REMOTE_RUN_B64='$remote_run_b64' RUN_ID_B64='$run_id_b64' bash -s" <<'REMOTE_CLEANUP'
set -euo pipefail
TESTBOX_REMOTE_ROOT="$(printf '%s' "${TESTBOX_REMOTE_ROOT_B64:?}" | base64 -d)"
REMOTE_RUN="$(printf '%s' "${REMOTE_RUN_B64:?}" | base64 -d)"
RUN_ID="$(printf '%s' "${RUN_ID_B64:-}" | base64 -d)"
assert_no_symlink_path() {
  local path="$1"
  while [ "$path" != "/" ] && [ "$path" != "." ]; do
    if [ -L "$path" ]; then
      return 2
    fi
    path="$(dirname "$path")"
  done
}
test ! -L "$TESTBOX_REMOTE_ROOT"
test "$(readlink -f -- "$TESTBOX_REMOTE_ROOT")" = "$TESTBOX_REMOTE_ROOT"
assert_no_symlink_path "$TESTBOX_REMOTE_ROOT"
test ! -L "$REMOTE_RUN"
assert_no_symlink_path "$REMOTE_RUN"
remote_run_real="$(readlink -m -- "$REMOTE_RUN")"
case "$remote_run_real" in
  "$TESTBOX_REMOTE_ROOT"/agents/*/runs/*|"$TESTBOX_REMOTE_ROOT"/workspaces/*/runs/*)
    if [ -n "$RUN_ID" ]; then
      test -f "$remote_run_real/.codex-run-id"
      test "$(cat -- "$remote_run_real/.codex-run-id")" = "$RUN_ID"
    fi
    rm -rf -- "$remote_run_real"
    ;;
  *) exit 2 ;;
esac
REMOTE_CLEANUP
}
