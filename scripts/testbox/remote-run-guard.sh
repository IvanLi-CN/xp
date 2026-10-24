#!/usr/bin/env bash

set -euo pipefail

: "${TESTBOX_REMOTE_ROOT:?set TESTBOX_REMOTE_ROOT before sourcing remote-run-guard.sh}"
: "${REMOTE_RUN:?set REMOTE_RUN before sourcing remote-run-guard.sh}"

REMOTE_RUN_GUARD_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

assert_remote_run() {
  local remote_run_real

  if ! RUN_ID="${REMOTE_RUN_ID:-}" python3 \
    "$REMOTE_RUN_GUARD_SCRIPT_DIR/remote-run-guard.py" verify "$TESTBOX_REMOTE_ROOT" "$REMOTE_RUN"; then
    return 2
  fi
  remote_run_real="$(python3 -c 'import os, sys; print(os.path.normpath(sys.argv[1]))' "$REMOTE_RUN")"
  REMOTE_RUN="$remote_run_real"
  export REMOTE_RUN
  if [ -n "${REMOTE_WORKSPACE:-}" ]; then
    REMOTE_WORKSPACE="$(dirname "$(dirname "$REMOTE_RUN")")"
    export REMOTE_WORKSPACE
  fi
}

remove_remote_run() {
  RUN_ID="${REMOTE_RUN_ID:-}" python3 \
    "$REMOTE_RUN_GUARD_SCRIPT_DIR/remote-run-guard.py" cleanup "$TESTBOX_REMOTE_ROOT" "$REMOTE_RUN"
}

assert_remote_run
