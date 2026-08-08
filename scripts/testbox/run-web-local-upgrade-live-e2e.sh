#!/usr/bin/env bash
set -euo pipefail

# High-cost live Web local upgrade regression suite on the shared testbox.
#
# The test starts a real xp server, triggers POST /api/admin/upgrade/start,
# lets a fake sudo/systemd boundary run xp-ops _upgrade-runner, and verifies:
# - success path restarts xp with the new binary/version
# - failure path rolls xp back and keeps the old service available
# - state written before upgrade remains readable after restart

TESTBOX="${TESTBOX:-codex-testbox}"
RUST_IMAGE="${RUST_IMAGE:-rust:1.96-bookworm}"

if REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)"; then
  :
else
  REPO_ROOT="$(pwd)"
fi
REPO_ROOT="$(python3 - "$REPO_ROOT" <<'PY'
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
)"

REPO_NAME="$(basename "$REPO_ROOT")"
PATH_HASH8="$(python3 - "$REPO_ROOT" <<'PY'
import hashlib, os, sys
p = os.path.realpath(sys.argv[1]).encode()
print(hashlib.sha256(p).hexdigest()[:8])
PY
)"

GIT_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo nogit)"
RUN_ID="$(date -u +%Y%m%d_%H%M%S)_$GIT_SHA"
WORKSPACE_SLUG="${REPO_NAME}__${PATH_HASH8}"

REMOTE_BASE="/srv/codex/workspaces/$USER"
REMOTE_WORKSPACE="$REMOTE_BASE/$WORKSPACE_SLUG"
REMOTE_RUN="$REMOTE_WORKSPACE/runs/$RUN_ID"

CONTAINER_RAW="codex_${WORKSPACE_SLUG}_web_upgrade_live_${RUN_ID}"
CONTAINER_NAME="$(python3 - "$CONTAINER_RAW" <<'PY'
import re, sys
s = sys.argv[1].lower()
s = re.sub(r'[^a-z0-9_.-]+', '_', s).strip('_.-')
print(s[:63] if len(s) > 63 else s)
PY
)"

REMOTE_RUN_B64="$(printf '%s' "$REMOTE_RUN" | base64 | tr -d '\n')"
REMOTE_WORKSPACE_B64="$(printf '%s' "$REMOTE_WORKSPACE" | base64 | tr -d '\n')"
CONTAINER_NAME_B64="$(printf '%s' "$CONTAINER_NAME" | base64 | tr -d '\n')"
RUST_IMAGE_B64="$(printf '%s' "$RUST_IMAGE" | base64 | tr -d '\n')"

echo "testbox=$TESTBOX"
echo "remote_run=$REMOTE_RUN"
echo "container=$CONTAINER_NAME"
echo "rust_image=$RUST_IMAGE"

CREATED_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
ssh -o BatchMode=yes "$TESTBOX" \
  "mkdir -p '$REMOTE_RUN' && cat > '$REMOTE_WORKSPACE/workspace.txt'" <<TXT
local_repo_root=$REPO_ROOT
created_utc=$CREATED_UTC
TXT

rsync -az --delete \
  --exclude '.git/' \
  --exclude 'node_modules/' \
  --exclude 'target/' \
  --exclude 'web/node_modules/' \
  --exclude 'web/dist/' \
  "$REPO_ROOT/" "$TESTBOX:$REMOTE_RUN/"

ssh -o BatchMode=yes "$TESTBOX" bash -s \
  "$REMOTE_RUN_B64" \
  "$REMOTE_WORKSPACE_B64" \
  "$CONTAINER_NAME_B64" \
  "$RUST_IMAGE_B64" <<'REMOTE'
set -euo pipefail

REMOTE_RUN="$(printf '%s' "${1:?}" | base64 -d)"
REMOTE_WORKSPACE="$(printf '%s' "${2:?}" | base64 -d)"
CONTAINER_NAME="$(printf '%s' "${3:?}" | base64 -d)"
RUST_IMAGE="$(printf '%s' "${4:?}" | base64 -d)"
CARGO_HOME_DIR="$REMOTE_WORKSPACE/cargo-home"
RUSTUP_HOME_DIR="$REMOTE_WORKSPACE/rustup-home"

cleanup() {
  set +e
  if [ -n "${CONTAINER_NAME:-}" ]; then
    docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
  fi
  if [ -n "${REMOTE_RUN:-}" ]; then
    rm -rf "$REMOTE_RUN" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

mkdir -p "$CARGO_HOME_DIR" "$RUSTUP_HOME_DIR"

docker run --rm \
  --name "$CONTAINER_NAME" \
  --label "codex.scope=web-local-upgrade-live" \
  --label "codex.remote_run=$REMOTE_RUN" \
  --cap-drop=ALL \
  --cap-add=CHOWN \
  --cap-add=DAC_OVERRIDE \
  --cap-add=FSETID \
  --cap-add=FOWNER \
  --cap-add=MKNOD \
  --cap-add=NET_RAW \
  --cap-add=SETGID \
  --cap-add=SETUID \
  --cap-add=SETPCAP \
  --cap-add=NET_BIND_SERVICE \
  --cap-add=SYS_CHROOT \
  --cap-add=KILL \
  --cap-add=AUDIT_WRITE \
  --security-opt no-new-privileges \
  --user "$(id -u):$(id -g)" \
  -e CARGO_HOME=/cargo-home \
  -e RUSTUP_HOME=/rustup-home \
  -e CARGO_HTTP_MULTIPLEXING=false \
  -e CARGO_NET_RETRY=8 \
  -e CARGO_TERM_COLOR=always \
  -v "$REMOTE_RUN:/workspace" \
  -v "$CARGO_HOME_DIR:/cargo-home" \
  -v "$RUSTUP_HOME_DIR:/rustup-home" \
  -w /workspace \
  "$RUST_IMAGE" \
  bash -lc '
set -euo pipefail

export PATH="/usr/local/cargo/bin:$PATH"
export TMPDIR="/workspace/tmp"
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
mkdir -p "$TMPDIR" web/dist
printf "%s\n" "<!doctype html><title>xp live upgrade e2e</title>" > web/dist/index.html

timeout 300 rustc --version
timeout 300 cargo --version
for attempt in 1 2 3; do
  if cargo fetch --locked; then
    break
  fi
  if [ "$attempt" = "3" ]; then
    exit 1
  fi
  sleep "$((attempt * 5))"
done

XP_OLD_VERSION=0.2.0
XP_NEW_VERSION=0.2.1
XP_LIVE_TARGET=/workspace/target-live
XP_ARTIFACTS=/workspace/tmp/live-artifacts

mkdir -p "$XP_ARTIFACTS/old" "$XP_ARTIFACTS/new"
XP_BUILD_VERSION="$XP_OLD_VERSION" CARGO_TARGET_DIR="$XP_LIVE_TARGET" \
  cargo build --bin xp --bin xp-ops
ln -f "$XP_LIVE_TARGET/debug/xp" "$XP_ARTIFACTS/old/xp"
ln -f "$XP_LIVE_TARGET/debug/xp-ops" "$XP_ARTIFACTS/old/xp-ops"

XP_BUILD_VERSION="$XP_NEW_VERSION" CARGO_TARGET_DIR="$XP_LIVE_TARGET" \
  cargo build --bin xp --bin xp-ops
ln -f "$XP_LIVE_TARGET/debug/xp" "$XP_ARTIFACTS/new/xp"
ln -f "$XP_LIVE_TARGET/debug/xp-ops" "$XP_ARTIFACTS/new/xp-ops"

random_port() {
  python3 - <<'"'"'PY'"'"'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
}

wait_version() {
  local base_url="$1"
  local expected="$2"
  local deadline=$((SECONDS + 40))
  while [ "$SECONDS" -lt "$deadline" ]; do
    local version
    version="$(curl -fsS "$base_url/api/cluster/info" \
      | python3 -c '"'"'import json,sys; print(json.load(sys.stdin)["xp_version"])'"'"' \
      2>/dev/null || true)"
    if [ "$version" = "$expected" ]; then
      return 0
    fi
    sleep 0.5
  done
  echo "timed out waiting for $base_url version $expected" >&2
  return 1
}

wait_upgrade_state() {
  local base_url="$1"
  local token="$2"
  local expected="$3"
  local deadline=$((SECONDS + 60))
  local last_state=""
  while [ "$SECONDS" -lt "$deadline" ]; do
    local state
    state="$(curl -fsS -H "Authorization: Bearer $token" \
      "$base_url/api/admin/upgrade/status" \
      | python3 -c '"'"'import json,sys; print(json.load(sys.stdin)["status"]["state"])'"'"' \
      2>/dev/null || true)"
    if [ -n "$state" ]; then
      last_state="$state"
    fi
    if [ "$state" = "$expected" ]; then
      return 0
    fi
    sleep 0.5
  done
  echo "timed out waiting for upgrade state $expected (last=$last_state)" >&2
  return 1
}

dump_case_debug() {
  local label="$1"
  echo "---- $label status.json ----" >&2
  [ -f "$XP_DATA_DIR/upgrade/status.json" ] && cat "$XP_DATA_DIR/upgrade/status.json" >&2 || true
  echo "---- $label runner.log ----" >&2
  [ -f "$RUNNER_LOG" ] && tail -200 "$RUNNER_LOG" >&2 || true
  echo "---- $label live.log ----" >&2
  [ -f "$LIVE_LOG" ] && tail -200 "$LIVE_LOG" >&2 || true
  echo "---- $label xp.log ----" >&2
  [ -f "$XP_LOG" ] && tail -200 "$XP_LOG" >&2 || true
  echo "---- $label installed binaries ----" >&2
  find "$TEST_ROOT/usr/local/bin" -maxdepth 1 -type f -printf "%f %s bytes\n" >&2 || true
}

write_release_fixture() {
  local dir="$1"
  local port="$2"
  mkdir -p "$dir/repos/IvanLi-CN/xp/releases/tags" "$dir/download"
  ln -f "$XP_ARTIFACTS/new/xp" "$dir/download/xp-linux-x86_64"
  ln -f "$XP_ARTIFACTS/new/xp-ops" "$dir/download/xp-ops-linux-x86_64"
  (
    cd "$dir/download"
    sha256sum xp-linux-x86_64 xp-ops-linux-x86_64 > checksums.txt
  )
  cat > "$dir/repos/IvanLi-CN/xp/releases/tags/v$XP_NEW_VERSION" <<JSON
{
  "tag_name": "v$XP_NEW_VERSION",
  "prerelease": false,
  "published_at": "2026-07-04T00:00:00Z",
  "assets": [
    {
      "name": "xp-linux-x86_64",
      "browser_download_url": "http://127.0.0.1:$port/download/xp-linux-x86_64"
    },
    {
      "name": "xp-ops-linux-x86_64",
      "browser_download_url": "http://127.0.0.1:$port/download/xp-ops-linux-x86_64"
    },
    {
      "name": "checksums.txt",
      "browser_download_url": "http://127.0.0.1:$port/download/checksums.txt"
    }
  ]
}
JSON
}

start_asset_server() {
  local dir="$1"
  local port="$2"
  python3 -m http.server "$port" --bind 127.0.0.1 --directory "$dir" \
    > "$dir/http.log" 2>&1 &
  echo "$!"
}

make_systemctl() {
  local bin_dir="$1"
  cat > "$bin_dir/systemctl" <<'"'"'SH'"'"'
#!/usr/bin/env bash
set -euo pipefail

echo "systemctl $*" >> "$LIVE_LOG"

if [ "$#" -eq 3 ] \
  && [ "$1" = "start" ] \
  && [ "$2" = "--no-block" ] \
  && [ "$3" = "xp-upgrade.service" ]; then
  env PATH="$FAKE_BIN:$PATH" \
    XP_OPS_GITHUB_API_BASE_URL="$ASSET_API_BASE" \
    XP_OPS_TEST_ENABLE_SERVICE=1 \
    "$XP_OPS_BIN" --root "$TEST_ROOT" _upgrade-runner --data-dir "$XP_DATA_DIR" \
    >> "$RUNNER_LOG" 2>&1 &
  echo "$!" > "$RUNNER_PID_FILE"
  exit 0
fi

if [ "$#" -eq 4 ] \
  && [ "$1" = "show" ] \
  && [ "$2" = "xp-upgrade.service" ]; then
  cat <<'EOF'
LoadState=loaded
ActiveState=active
SubState=running
Result=success
ExecMainStatus=0
EOF
  exit 0
fi

if [ "$#" -eq 3 ] \
  && [ "$1" = "is-active" ] \
  && [ "$2" = "--quiet" ] \
  && [ "$3" = "xp.service" ] \
  && [ -f "$XP_PID_FILE" ] \
  && kill -0 "$(cat "$XP_PID_FILE")" >/dev/null 2>&1; then
  exit 0
fi

if [ "$#" -eq 3 ] \
  && [ "$1" = "is-active" ] \
  && [ "$2" = "--quiet" ] \
  && [ "$3" = "xray.service" ]; then
  exit 0
fi

if [ "$#" -eq 2 ] && [ "$1" = "restart" ] && [ "$2" = "xray.service" ]; then
  exit 0
fi

if [ "$#" -eq 2 ] && [ "$1" = "restart" ] && [ "$2" = "xp.service" ]; then
  if [ -f "$RESTART_FAIL_FILE" ]; then
    exit 1
  fi
  if [ -f "$XP_PID_FILE" ]; then
    kill "$(cat "$XP_PID_FILE")" >/dev/null 2>&1 || true
  fi
  env PATH="$FAKE_BIN:$PATH" \
    XP_UPGRADE_TEST_FORCE_HOST_TRIGGER=systemd \
    XP_UPGRADE_TEST_SYSTEMD_TRIGGER_PATH="$TEST_ROOT/usr/local/libexec/xp-upgrade-trigger" \
    XP_DATA_DIR="$XP_DATA_DIR" \
    XP_BIND="$XP_BIND" \
    XP_API_BASE_URL="$XP_API_BASE_URL" \
    XP_ACCESS_HOST="$XP_ACCESS_HOST" \
    XP_ADMIN_TOKEN_HASH="$XP_ADMIN_TOKEN_HASH" \
    XP_CLOUDFLARED_MONITOR_MODE=none \
    XP_IP_GEO_ENABLED=false \
    "$XP_BIN" run >> "$XP_LOG" 2>&1 &
  echo "$!" > "$XP_PID_FILE"
  exit 0
fi

exit 99
SH
  chmod +x "$bin_dir/systemctl"
}

make_sudo() {
  local bin_dir="$1"
  cat > "$bin_dir/sudo" <<'"'"'SH'"'"'
#!/usr/bin/env bash
set -euo pipefail

echo "sudo $*" >> "$LIVE_LOG"

if [ "$#" -ge 1 ] && [ "$1" = "-n" ]; then
  shift
fi

if [ "$#" -ge 1 ] && [ "$1" = "-l" ]; then
  shift
  if [ "$#" -eq 1 ] && [ "$1" = "$TEST_ROOT/usr/local/libexec/xp-upgrade-trigger" ]; then
    exit 0
  fi
  exit 1
fi

exec "$@"
SH
  chmod +x "$bin_dir/sudo"
}

install_systemd_upgrade_helper() {
  mkdir -p "$TEST_ROOT/usr/local/libexec"
  cat > "$TEST_ROOT/usr/local/libexec/xp-upgrade-trigger" <<SH
#!/bin/sh
set -eu
case "\${1:-}" in
  "") ;;
  --check) exit 0 ;;
  *) echo "usage: xp-upgrade-trigger [--check]" >&2; exit 64 ;;
esac
exec "$FAKE_BIN/systemctl" start --no-block xp-upgrade.service
SH
  chmod +x "$TEST_ROOT/usr/local/libexec/xp-upgrade-trigger"
}

start_xp() {
  env PATH="$FAKE_BIN:$PATH" \
    XP_UPGRADE_TEST_FORCE_HOST_TRIGGER=systemd \
    XP_UPGRADE_TEST_SYSTEMD_TRIGGER_PATH="$TEST_ROOT/usr/local/libexec/xp-upgrade-trigger" \
    XP_DATA_DIR="$XP_DATA_DIR" \
    XP_BIND="$XP_BIND" \
    XP_API_BASE_URL="$XP_API_BASE_URL" \
    XP_ACCESS_HOST="$XP_ACCESS_HOST" \
    XP_ADMIN_TOKEN_HASH="$XP_ADMIN_TOKEN_HASH" \
    XP_CLOUDFLARED_MONITOR_MODE=none \
    XP_IP_GEO_ENABLED=false \
    "$XP_BIN" run >> "$XP_LOG" 2>&1 &
  echo "$!" > "$XP_PID_FILE"
}

stop_xp() {
  if [ -f "$XP_PID_FILE" ]; then
    kill "$(cat "$XP_PID_FILE")" >/dev/null 2>&1 || true
  fi
}

prepare_case() {
  local case_dir="$1"
  local bind_port="$2"
  local asset_port="$3"
  TEST_ROOT="$case_dir/root"
  XP_DATA_DIR="$case_dir/data"
  ASSET_DIR="$case_dir/assets"
  FAKE_BIN="$case_dir/bin"
  XP_LOG="$case_dir/xp.log"
  RUNNER_LOG="$case_dir/runner.log"
  LIVE_LOG="$case_dir/live.log"
  RESTART_FAIL_FILE="$case_dir/restart-fails"
  XP_PID_FILE="$case_dir/xp.pid"
  RUNNER_PID_FILE="$case_dir/runner.pid"
  XP_BIND="127.0.0.1:$bind_port"
  XP_API_BASE_URL="http://127.0.0.1:$bind_port"
  XP_ACCESS_HOST="127.0.0.1"
  ASSET_API_BASE="http://127.0.0.1:$asset_port"
  XP_BIN="$TEST_ROOT/usr/local/bin/xp"
  XP_OPS_BIN="$TEST_ROOT/usr/local/bin/xp-ops"
  ADMIN_TOKEN="test-admin-token-for-live-upgrade-e2e"

  mkdir -p "$TEST_ROOT/usr/local/bin" "$TEST_ROOT/etc/xray" "$FAKE_BIN"
  ln -f "$XP_ARTIFACTS/old/xp" "$XP_BIN"
  ln -f "$XP_ARTIFACTS/old/xp-ops" "$XP_OPS_BIN"
  chmod +x "$XP_BIN" "$XP_OPS_BIN"
  printf "%s\n" "{\"policy\":{\"levels\":{\"0\":{\"statsUserUplink\":true}}}}" \
    > "$TEST_ROOT/etc/xray/config.json"
  "$XP_OPS_BIN" --root "$TEST_ROOT" admin-token set --token "$ADMIN_TOKEN" --quiet
  set -a
  . "$TEST_ROOT/etc/xp/xp.env"
  set +a
  export TEST_ROOT XP_DATA_DIR ASSET_DIR FAKE_BIN XP_LOG RUNNER_LOG LIVE_LOG
  export RESTART_FAIL_FILE
  export XP_PID_FILE RUNNER_PID_FILE XP_BIND XP_API_BASE_URL XP_ACCESS_HOST
  export ASSET_API_BASE XP_BIN XP_OPS_BIN ADMIN_TOKEN XP_ADMIN_TOKEN_HASH
  make_systemctl "$FAKE_BIN"
  make_sudo "$FAKE_BIN"
  install_systemd_upgrade_helper
}

run_success_case() {
  local case_dir="/workspace/tmp/live-success"
  local bind_port
  local asset_port
  bind_port="$(random_port)"
  asset_port="$(random_port)"
  prepare_case "$case_dir" "$bind_port" "$asset_port"
  write_release_fixture "$ASSET_DIR" "$asset_port"
  local asset_pid
  asset_pid="$(start_asset_server "$ASSET_DIR" "$asset_port")"
  trap "kill $asset_pid >/dev/null 2>&1 || true; stop_xp" RETURN

  "$XP_BIN" init
  start_xp
  wait_version "$XP_API_BASE_URL" "$XP_OLD_VERSION" || {
    dump_case_debug success-startup
    return 1
  }

  local user_id
  user_id="$(curl -sS --fail-with-body -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"display_name\":\"upgrade sentinel\"}" \
    "$XP_API_BASE_URL/api/admin/users" \
    | python3 -c '"'"'import json,sys; print(json.load(sys.stdin)["user_id"])'"'"')" || {
    dump_case_debug success-create-user
    return 1
  }

  local start_body="$case_dir/start-upgrade.body"
  curl -sS --fail-with-body -o "$start_body" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"target_tag\":\"v$XP_NEW_VERSION\"}" \
    "$XP_API_BASE_URL/api/admin/upgrade/start" || {
    cat "$start_body" >&2 || true
    dump_case_debug success-start-upgrade
    return 1
  }

  wait_upgrade_state "$XP_API_BASE_URL" "$ADMIN_TOKEN" "succeeded" || {
    dump_case_debug success
    return 1
  }
  wait_version "$XP_API_BASE_URL" "$XP_NEW_VERSION" || {
    dump_case_debug success-version
    return 1
  }
  curl -fsS -H "Authorization: Bearer $ADMIN_TOKEN" \
    "$XP_API_BASE_URL/api/admin/users/$user_id" \
    | python3 -c '"'"'
import json
import sys
assert json.load(sys.stdin)["display_name"] == "upgrade sentinel"
'"'"'
  grep -Fxq "sudo -n $TEST_ROOT/usr/local/libexec/xp-upgrade-trigger --check" "$LIVE_LOG"
  grep -Fxq "sudo -n -l $TEST_ROOT/usr/local/libexec/xp-upgrade-trigger" "$LIVE_LOG"
  grep -Fxq "sudo -n $TEST_ROOT/usr/local/libexec/xp-upgrade-trigger" "$LIVE_LOG"
  grep -Fxq "systemctl start --no-block xp-upgrade.service" "$LIVE_LOG"
  grep -Fxq "systemctl restart xp.service" "$LIVE_LOG"
  stop_xp
  kill "$asset_pid" >/dev/null 2>&1 || true
  rm -rf "$case_dir"
}

run_rollback_case() {
  local case_dir="/workspace/tmp/live-rollback"
  local bind_port
  local asset_port
  bind_port="$(random_port)"
  asset_port="$(random_port)"
  prepare_case "$case_dir" "$bind_port" "$asset_port"
  write_release_fixture "$ASSET_DIR" "$asset_port"
  local asset_pid
  asset_pid="$(start_asset_server "$ASSET_DIR" "$asset_port")"
  trap "kill $asset_pid >/dev/null 2>&1 || true; stop_xp" RETURN

  "$XP_BIN" init
  start_xp
  wait_version "$XP_API_BASE_URL" "$XP_OLD_VERSION" || {
    dump_case_debug rollback-startup
    return 1
  }
  # A durable v2 epoch intentionally blocks rollback to a v1 binary. This fixture
  # exercises the ordinary legacy rollback path required by the upgrade contract.
  rm -f "$XP_DATA_DIR/mesh/internal-auth-v2.json"
  touch "$RESTART_FAIL_FILE"

  local start_body="$case_dir/start-upgrade.body"
  curl -sS --fail-with-body -o "$start_body" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"target_tag\":\"v$XP_NEW_VERSION\"}" \
    "$XP_API_BASE_URL/api/admin/upgrade/start" || {
    cat "$start_body" >&2 || true
    dump_case_debug rollback-start-upgrade
    return 1
  }

  wait_upgrade_state "$XP_API_BASE_URL" "$ADMIN_TOKEN" "failed" || {
    dump_case_debug rollback
    return 1
  }
  wait_version "$XP_API_BASE_URL" "$XP_OLD_VERSION" || {
    dump_case_debug rollback-version
    return 1
  }
  "$XP_BIN" --version | grep -q "$XP_OLD_VERSION"
  ! find "$TEST_ROOT/usr/local/bin" -type f \( -name '*.bak.*' -o -name '*.failed.*' \) -print -quit | grep -q . || {
    dump_case_debug rollback-artifacts
    return 1
  }
  test -s "$XP_DATA_DIR/upgrade/diagnostics.json" || {
    dump_case_debug rollback-diagnostics
    return 1
  }
  test "$(wc -c < "$XP_DATA_DIR/upgrade/diagnostics.json")" -le 8192 || {
    dump_case_debug rollback-diagnostics-size
    return 1
  }
}

run_success_case
run_rollback_case
rm -rf "$XP_LIVE_TARGET" "$XP_ARTIFACTS"
XP_MIGRATION_TARGET=/workspace/target-migrations
CARGO_TARGET_DIR="$XP_MIGRATION_TARGET" \
  cargo test --lib state::tests::load_or_init_migrates_v1_state_json_public_domain_to_access_host
CARGO_TARGET_DIR="$XP_MIGRATION_TARGET" \
  cargo test --lib state::tests::load_or_init_recovers_when_state_is_v10_but_usage_is_v1
CARGO_TARGET_DIR="$XP_MIGRATION_TARGET" \
  cargo test --lib raft::storage::file::tests::install_snapshot_migrates_legacy_grants_state_to_v10
  '
REMOTE
