#!/usr/bin/env bash

set -euo pipefail

: "${TESTBOX_REMOTE_ROOT:?set TESTBOX_REMOTE_ROOT before sourcing remote-run-guard.sh}"
: "${REMOTE_RUN:?set REMOTE_RUN before sourcing remote-run-guard.sh}"

assert_no_symlink_path() {
  local path="$1"
  while [ "$path" != "/" ] && [ "$path" != "." ]; do
    if [ -L "$path" ]; then
      echo "remote run path contains a symlink: $path" >&2
      return 2
    fi
    path="$(dirname -- "$path")"
  done
}

assert_remote_run() {
  local remote_root_real remote_run_real

  test ! -L "$TESTBOX_REMOTE_ROOT"
  remote_root_real="$(readlink -f -- "$TESTBOX_REMOTE_ROOT")"
  test "$remote_root_real" = "$TESTBOX_REMOTE_ROOT"
  assert_no_symlink_path "$TESTBOX_REMOTE_ROOT"
  test ! -L "$REMOTE_RUN"
  remote_run_real="$(readlink -f -- "$REMOTE_RUN")"
  case "$remote_run_real" in
    "$remote_root_real"/agents/*/runs/*|"$remote_root_real"/workspaces/*/runs/*) ;;
    *)
      echo "remote run path escaped the configured remote root: $REMOTE_RUN" >&2
      return 2
      ;;
  esac
  assert_no_symlink_path "$remote_run_real"
  REMOTE_RUN="$remote_run_real"
  export REMOTE_RUN
}

assert_remote_run
