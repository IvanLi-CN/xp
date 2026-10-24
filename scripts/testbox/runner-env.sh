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

testbox_remote_root_preflight() {
  ssh -o BatchMode=yes "$TESTBOX" \
    "set -euo pipefail; root='$TESTBOX_REMOTE_ROOT'; mkdir -p -- \"\$root\"; test -d \"\$root\"; test ! -L \"\$root\"; test \"\$(readlink -f -- \"\$root\")\" = \"\$root\""
}

testbox_remote_run_guard() {
  local remote_run="$1"
  ssh -o BatchMode=yes "$TESTBOX" \
    "TESTBOX_REMOTE_ROOT_B64=\$(printf '%s' '$TESTBOX_REMOTE_ROOT' | base64 | tr -d '\\n') REMOTE_RUN_B64=\$(printf '%s' '$remote_run' | base64 | tr -d '\\n') bash -s" <<'REMOTE_RUN_GUARD'
set -euo pipefail
TESTBOX_REMOTE_ROOT="$(printf '%s' "${TESTBOX_REMOTE_ROOT_B64:?}" | base64 -d)"
REMOTE_RUN="$(printf '%s' "${REMOTE_RUN_B64:?}" | base64 -d)"
assert_no_symlink_path() {
  local path="$1"
  while [ "$path" != "/" ] && [ "$path" != "." ]; do
    test ! -L "$path"
    path="$(dirname -- "$path")"
  done
}
test ! -L "$TESTBOX_REMOTE_ROOT"
test "$(readlink -f -- "$TESTBOX_REMOTE_ROOT")" = "$TESTBOX_REMOTE_ROOT"
assert_no_symlink_path "$TESTBOX_REMOTE_ROOT"
test ! -L "$REMOTE_RUN"
remote_run_real="$(readlink -f -- "$REMOTE_RUN")"
case "$remote_run_real" in
  "$TESTBOX_REMOTE_ROOT"/agents/*/runs/*|"$TESTBOX_REMOTE_ROOT"/workspaces/*/runs/*) ;;
  *) exit 2 ;;
esac
assert_no_symlink_path "$remote_run_real"
REMOTE_RUN="$remote_run_real"
REMOTE_RUN_GUARD
}
