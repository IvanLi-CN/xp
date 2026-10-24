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
    path="$(dirname "$path")"
  done
}

assert_remote_run() {
  local remote_root_real remote_run_real

  if [ -L "$TESTBOX_REMOTE_ROOT" ]; then return 2; fi
  remote_root_real="$(readlink -f -- "$TESTBOX_REMOTE_ROOT")"
  if [ "$remote_root_real" != "$TESTBOX_REMOTE_ROOT" ]; then return 2; fi
  if ! assert_no_symlink_path "$TESTBOX_REMOTE_ROOT"; then return 2; fi
  if [ -L "$REMOTE_RUN" ]; then return 2; fi
  if ! assert_no_symlink_path "$REMOTE_RUN"; then return 2; fi
  remote_run_real="$(readlink -m -- "$REMOTE_RUN")"
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
  if [ -n "${REMOTE_RUN_ID:-}" ]; then
    if [ ! -f "$REMOTE_RUN/.codex-run-id" ]; then return 2; fi
    if [ "$(cat -- "$REMOTE_RUN/.codex-run-id")" != "$REMOTE_RUN_ID" ]; then return 2; fi
  fi
  if [ -n "${REMOTE_WORKSPACE:-}" ]; then
    REMOTE_WORKSPACE="$(dirname "$(dirname "$REMOTE_RUN")")"
    export REMOTE_WORKSPACE
  fi
}

assert_remote_run
