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
TESTBOX_REMOTE_GUARD_HELPER="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/remote-run-guard.py"
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

testbox_bounded_name() {
  python3 - "$1" <<'PY'
import hashlib
import re
import sys

raw = sys.argv[1]
safe = re.sub(r"[^a-z0-9_-]+", "_", raw.lower()).strip("_-")
digest = hashlib.sha256(raw.encode()).hexdigest()[:12]
prefix = safe[: 63 - len(digest) - 1].rstrip("_-") or "run"
print(f"{prefix}_{digest}")
PY
}

testbox_remote_root_preflight() {
  ssh -o BatchMode=yes "$TESTBOX" \
    "set -euo pipefail; root='$TESTBOX_REMOTE_ROOT'; mkdir -p -- \"\$root\"; test -d \"\$root\"; test ! -L \"\$root\"; test \"\$(readlink -f -- \"\$root\")\" = \"\$root\""
}

testbox_remote_run_create() {
  local remote_run="$1"
  local run_id="${2:-${RUN_ID:-$(basename "$remote_run")}}"
  local root_b64 remote_run_b64 run_id_b64
  root_b64="$(printf '%s' "$TESTBOX_REMOTE_ROOT" | base64 | tr -d '\n')"
  remote_run_b64="$(printf '%s' "$remote_run" | base64 | tr -d '\n')"
  run_id_b64="$(printf '%s' "$run_id" | base64 | tr -d '\n')"
  ssh -o BatchMode=yes "$TESTBOX" \
    "TESTBOX_REMOTE_ROOT_B64='$root_b64' REMOTE_RUN_B64='$remote_run_b64' RUN_ID_B64='$run_id_b64' python3 - guard" < "$TESTBOX_REMOTE_GUARD_HELPER"
}

testbox_remote_run_guard() {
  testbox_remote_run_create "$@"
}

testbox_remote_run_verify() {
  local remote_run="$1"
  local run_id="${2:-${RUN_ID:-$(basename "$remote_run")}}"
  local root_b64 remote_run_b64 run_id_b64
  root_b64="$(printf '%s' "$TESTBOX_REMOTE_ROOT" | base64 | tr -d '\n')"
  remote_run_b64="$(printf '%s' "$remote_run" | base64 | tr -d '\n')"
  run_id_b64="$(printf '%s' "$run_id" | base64 | tr -d '\n')"
  ssh -o BatchMode=yes "$TESTBOX" \
    "TESTBOX_REMOTE_ROOT_B64='$root_b64' REMOTE_RUN_B64='$remote_run_b64' RUN_ID_B64='$run_id_b64' python3 - verify" < "$TESTBOX_REMOTE_GUARD_HELPER" >/dev/null
}

testbox_remote_cleanup() {
  local remote_run="$1"
  local run_id="${2:-}"
  local root_b64 remote_run_b64 run_id_b64
  root_b64="$(printf '%s' "$TESTBOX_REMOTE_ROOT" | base64 | tr -d '\n')"
  remote_run_b64="$(printf '%s' "$remote_run" | base64 | tr -d '\n')"
  run_id_b64="$(printf '%s' "$run_id" | base64 | tr -d '\n')"
  ssh -o BatchMode=yes "$TESTBOX" \
    "TESTBOX_REMOTE_ROOT_B64='$root_b64' REMOTE_RUN_B64='$remote_run_b64' RUN_ID_B64='$run_id_b64' python3 - cleanup" < "$TESTBOX_REMOTE_GUARD_HELPER"
}
