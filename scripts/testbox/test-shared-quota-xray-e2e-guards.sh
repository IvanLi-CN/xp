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
  'TESTBOX: set TESTBOX to an authorized SSH target' \
  env XP_TESTBOX_REMOTE_ROOT=/tmp/xp-testbox-guard "$RUNNER"

expect_failure \
  'XP_TESTBOX_REMOTE_ROOT: set XP_TESTBOX_REMOTE_ROOT to a writable remote root' \
  env TESTBOX=ssh-target "$RUNNER"

expect_failure \
  'XP_TESTBOX_REMOTE_ROOT must contain only safe path characters' \
  env \
    TESTBOX=ssh-target \
    XP_TESTBOX_REMOTE_ROOT='/tmp/xp-testbox guard' \
    "$RUNNER"

expect_failure \
  'USER must contain only safe path characters' \
  env \
    TESTBOX=ssh-target \
    USER='unsafe user' \
    XP_TESTBOX_REMOTE_ROOT=/tmp/xp-testbox-guard \
    "$RUNNER"

expect_failure \
  'XP_TESTBOX_CARGO_CACHE_ROOT and XP_TESTBOX_CARGO_HOME are required' \
  env \
    TESTBOX=ssh-target \
    XP_TESTBOX_REMOTE_ROOT=/tmp/xp-testbox-guard \
    XP_RUN_MESH_RESOURCE=1 \
    "$RUNNER"

expect_failure \
  'XP_TESTBOX_CACHE_SMOKE must be 0 or 1' \
  env \
    TESTBOX=ssh-target \
    XP_TESTBOX_REMOTE_ROOT=/tmp/xp-testbox-guard \
    XP_RUN_MESH_RESOURCE=1 \
    XP_TESTBOX_CARGO_CACHE_ROOT=/tmp/xp-testbox-guard/agents/test/cache \
    XP_TESTBOX_CARGO_HOME=/tmp/xp-testbox-guard/cargo-home \
    XP_TESTBOX_CACHE_SMOKE=2 \
    "$RUNNER"

expect_failure \
  'the final Mesh resource gate requires XP_MESH_RESOURCE_DURATION_SECS=900' \
  env \
    TESTBOX=ssh-target \
    XP_TESTBOX_REMOTE_ROOT=/tmp/xp-testbox-guard \
    XP_RUN_MESH_RESOURCE=1 \
    XP_TESTBOX_CARGO_CACHE_ROOT=/tmp/xp-testbox-guard/agents/test/cache \
    XP_TESTBOX_CARGO_HOME=/tmp/xp-testbox-guard/cargo-home \
    XP_MESH_RESOURCE_DURATION_SECS=600 \
    "$RUNNER"

expect_failure \
  'cache smoke requires XP_MESH_RESOURCE_DURATION_SECS=600' \
  env \
    TESTBOX=ssh-target \
    XP_TESTBOX_REMOTE_ROOT=/tmp/xp-testbox-guard \
    XP_RUN_MESH_RESOURCE=1 \
    XP_TESTBOX_CARGO_CACHE_ROOT=/tmp/xp-testbox-guard/agents/test/cache \
    XP_TESTBOX_CARGO_HOME=/tmp/xp-testbox-guard/cargo-home \
    XP_TESTBOX_CACHE_SMOKE=1 \
    XP_MESH_RESOURCE_DURATION_SECS=900 \
    "$RUNNER"

(
  set -euo pipefail
  guard_root="$(mktemp -d "${TMPDIR:-/tmp}/xp-testbox-remote-guard.XXXXXX")"
  guard_root="$(cd "$guard_root" && pwd -P)"
  readlink() {
    case "${1:-}" in
      -f|-m)
        shift
        [ "${1:-}" = "--" ] && shift
        python3 -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$1"
        ;;
      *) command readlink "$@" ;;
    esac
  }
  export -f readlink
  ssh() {
    shift 3
    bash -c "$1"
  }
  TESTBOX=local
  XP_TESTBOX_REMOTE_ROOT="$guard_root/root"
  USER=ivan
  # shellcheck source=runner-env.sh
  source "$SCRIPT_DIR/runner-env.sh"
  mkdir -p "$guard_root/outside"
  ln -s "$guard_root/outside" "$guard_root/root-alias"
  bad_root_b64="$(printf '%s' "$guard_root/root-alias/nested" | base64 | tr -d '\n')"
  if TESTBOX_REMOTE_ROOT_B64="$bad_root_b64" python3 "$SCRIPT_DIR/remote-run-guard.py" ensure-root; then
    echo 'expected symlinked root ancestor guard to fail' >&2
    exit 1
  fi
  test ! -e "$guard_root/outside/nested"
  long_name_raw="$(printf 'repository%.0s' {1..20})"
  long_name="$(testbox_bounded_name "$long_name_raw")"
  test "${#long_name}" -eq 63
  test "${long_name##*_}" = "$(python3 - "$long_name_raw" <<'PY'
import hashlib
import sys
print(hashlib.sha256(sys.argv[1].encode()).hexdigest()[:12])
PY
)"
  testbox_remote_root_preflight
  guarded_run="$(testbox_remote_run_create "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/repo/runs/run-1")"
  test "$guarded_run" = "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/repo/runs/run-1"
  test "$(cat "$guarded_run/.codex-run-id")" = run-1
  testbox_remote_cleanup "$guarded_run" run-1
  test ! -e "$guarded_run"
  python3 - "$guard_root" "$SCRIPT_DIR/remote-run-guard.py" <<'PY'
import importlib.util
import os
import pathlib
import sys

root = pathlib.Path(sys.argv[1]) / "rename-noreplace"
root.mkdir()
(root / "source").mkdir()
(root / "target").mkdir()
spec = importlib.util.spec_from_file_location("remote_run_guard", sys.argv[2])
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)
parent_fd = os.open(root, module.OPEN_FLAGS)
try:
    assert not module.rename_noreplace(parent_fd, "source", "target")
finally:
    os.close(parent_fd)
assert (root / "source").is_dir()
assert (root / "target").is_dir()
PY
  guarded_run="$(testbox_remote_run_create "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/repo/runs/run-3")"
  test "$(cat "$guarded_run/.codex-run-id")" = run-3
  if TESTBOX_REMOTE_ROOT="$XP_TESTBOX_REMOTE_ROOT" \
    REMOTE_RUN="$guarded_run" \
    REMOTE_RUN_ID=run-2 \
    REMOTE_WORKSPACE="$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/repo" \
    bash "$SCRIPT_DIR/remote-run-guard.sh"; then
    echo 'expected remote marker guard to fail' >&2
    exit 1
  fi
  testbox_remote_cleanup "$guarded_run" run-3
  test ! -e "$guarded_run"
  mkdir -p "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/insecure"
  chmod 0777 "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/insecure"
  if testbox_remote_run_create \
    "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/insecure/runs/run-insecure"; then
    echo 'expected writable remote run parent guard to fail' >&2
    exit 1
  fi
  claim_dir="$XP_TESTBOX_REMOTE_ROOT/agents/.shared-testbox-subnet-claims/10_203_0_0_24"
  mkdir -p "$claim_dir"
  python3 - "$XP_TESTBOX_REMOTE_ROOT" "$claim_dir" "$SCRIPT_DIR/remote-run-guard.py" <<'PY'
import fcntl
import os
import subprocess
import sys

root, claim, helper = sys.argv[1:]
lock_path = os.path.join(root, "agents", ".shared-testbox-subnet-claims", ".codex-run-lock")
with open(lock_path, "a+") as lock_file:
    fcntl.flock(lock_file, fcntl.LOCK_EX)
    subprocess.run(
        ["python3", helper, "claim-cleanup-locked", root, claim],
        check=True,
    )
assert not os.path.exists(claim)
PY
  rm -rf "$XP_TESTBOX_REMOTE_ROOT/agents"
  mkdir -p "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan"
  mkdir -p "$guard_root/outside"
  rm -rf "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/repo"
  ln -s "$guard_root/outside" "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/repo"
  test -L "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/repo"
  if testbox_remote_run_create "$XP_TESTBOX_REMOTE_ROOT/workspaces/ivan/repo/runs/run-2"; then
    echo 'expected intermediate symlink guard to fail' >&2
    exit 1
  fi
  mkdir -p "$guard_root/agents-outside"
  ln -s "$guard_root/agents-outside" "$XP_TESTBOX_REMOTE_ROOT/agents"
  if testbox_remote_run_create "$XP_TESTBOX_REMOTE_ROOT/agents/ivan/runs/run-4"; then
    echo 'expected agents symlink guard to fail' >&2
    exit 1
  fi
)

printf 'shared testbox runner guard tests: passed\n'
