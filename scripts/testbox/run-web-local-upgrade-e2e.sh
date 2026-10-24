#!/usr/bin/env bash
set -euo pipefail

# High-cost Web local upgrade regression suite on the shared testbox.
#
# This follows the shared-testbox-runner rules:
# - per-run isolation under the caller-provided remote root
# - no local Docker usage
# - no global Docker cleanup
# - docker run uses cap-drop=ALL for LXC compatibility
# - cleanup only removes this run's container and run directory

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=runner-env.sh
source "$SCRIPT_DIR/runner-env.sh"
RUST_IMAGE="${RUST_IMAGE:-rust:1.96-bookworm}"
ALPINE_IMAGE="${ALPINE_IMAGE:-alpine:3.22}"

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
testbox_validate_component repository "$REPO_NAME"
PATH_HASH8="$(python3 - "$REPO_ROOT" <<'PY'
import hashlib, os, sys
p = os.path.realpath(sys.argv[1]).encode()
print(hashlib.sha256(p).hexdigest()[:8])
PY
)"

GIT_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo nogit)"
RUN_NONCE="$(od -An -N4 -tu4 /dev/urandom | tr -d ' ')"
RUN_ID="$(date -u +%Y%m%d_%H%M%S)_${GIT_SHA}_${RUN_NONCE}"
WORKSPACE_SLUG="${REPO_NAME}__${PATH_HASH8}"

REMOTE_BASE="$TESTBOX_REMOTE_ROOT/workspaces/$USER"
REMOTE_WORKSPACE="$REMOTE_BASE/$WORKSPACE_SLUG"
REMOTE_RUN="$REMOTE_WORKSPACE/runs/$RUN_ID"
REMOTE_RUN_CREATED=0
cleanup_remote_run() {
  local status="${1:-$?}"
  set +e
  if [ "$REMOTE_RUN_CREATED" = "1" ] && [ "$status" -eq 0 ]; then
    testbox_remote_cleanup "$REMOTE_RUN" "$RUN_ID" >/dev/null 2>&1 || true
  fi
  return "$status"
}
on_signal() {
  local status="$1"
  trap - EXIT INT TERM
  cleanup_remote_run "$status"
  exit "$status"
}
trap cleanup_remote_run EXIT
trap 'on_signal 130' INT
trap 'on_signal 143' TERM
testbox_remote_root_preflight
REMOTE_RUN_CREATED=1
REMOTE_RUN="$(testbox_remote_run_create "$REMOTE_RUN")"
REMOTE_WORKSPACE="$(dirname "$(dirname "$REMOTE_RUN")")"

CONTAINER_RAW="codex_${WORKSPACE_SLUG}_web_upgrade_${RUN_ID}"
CONTAINER_NAME="$(testbox_bounded_name "$CONTAINER_RAW")"
OPENRC_CONTAINER_RAW="codex_${WORKSPACE_SLUG}_openrc_upgrade_${RUN_ID}"
OPENRC_CONTAINER_NAME="$(testbox_bounded_name "$OPENRC_CONTAINER_RAW")"

REMOTE_RUN_B64="$(printf '%s' "$REMOTE_RUN" | base64 | tr -d '\n')"
TESTBOX_REMOTE_ROOT_B64="$(printf '%s' "$TESTBOX_REMOTE_ROOT" | base64 | tr -d '\n')"
REMOTE_WORKSPACE_B64="$(printf '%s' "$REMOTE_WORKSPACE" | base64 | tr -d '\n')"
CONTAINER_NAME_B64="$(printf '%s' "$CONTAINER_NAME" | base64 | tr -d '\n')"
RUST_IMAGE_B64="$(printf '%s' "$RUST_IMAGE" | base64 | tr -d '\n')"
OPENRC_CONTAINER_NAME_B64="$(printf '%s' "$OPENRC_CONTAINER_NAME" | base64 | tr -d '\n')"
ALPINE_IMAGE_B64="$(printf '%s' "$ALPINE_IMAGE" | base64 | tr -d '\n')"
RUN_ID_B64="$(printf '%s' "$RUN_ID" | base64 | tr -d '\n')"

echo "testbox=$TESTBOX"
echo "remote_run=$REMOTE_RUN"
echo "container=$CONTAINER_NAME"
echo "rust_image=$RUST_IMAGE"
echo "openrc_container=$OPENRC_CONTAINER_NAME"
echo "alpine_image=$ALPINE_IMAGE"

CREATED_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
ssh -o BatchMode=yes "$TESTBOX" \
  "cat > '$REMOTE_WORKSPACE/workspace.txt'" <<TXT
local_repo_root=$REPO_ROOT
created_utc=$CREATED_UTC
TXT

rsync -az --delete \
  --exclude '.codex-run-id' \
  --exclude '.git/' \
  --exclude 'node_modules/' \
  --exclude 'target/' \
  --exclude 'web/node_modules/' \
  --exclude 'web/dist/' \
  "$REPO_ROOT/" "$TESTBOX:$REMOTE_RUN/"

ssh -o BatchMode=yes "$TESTBOX" bash -s \
  "$REMOTE_RUN_B64" \
  "$TESTBOX_REMOTE_ROOT_B64" \
  "$REMOTE_WORKSPACE_B64" \
  "$CONTAINER_NAME_B64" \
  "$RUST_IMAGE_B64" \
  "$OPENRC_CONTAINER_NAME_B64" \
  "$ALPINE_IMAGE_B64" \
  "$RUN_ID_B64" <<'REMOTE'
set -euo pipefail

REMOTE_RUN="$(printf '%s' "${1:?}" | base64 -d)"
TESTBOX_REMOTE_ROOT="$(printf '%s' "${2:?}" | base64 -d)"
REMOTE_WORKSPACE="$(printf '%s' "${3:?}" | base64 -d)"
CONTAINER_NAME="$(printf '%s' "${4:?}" | base64 -d)"
RUST_IMAGE="$(printf '%s' "${5:?}" | base64 -d)"
OPENRC_CONTAINER_NAME="$(printf '%s' "${6:?}" | base64 -d)"
ALPINE_IMAGE="$(printf '%s' "${7:?}" | base64 -d)"
RUN_ID="$(printf '%s' "${8:?}" | base64 -d)"
REMOTE_RUN_ID="$RUN_ID"
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
  "$TESTBOX_REMOTE_ROOT"/agents/*/runs/*|"$TESTBOX_REMOTE_ROOT"/workspaces/*/runs/*) ;;
  *) exit 2 ;;
esac
REMOTE_RUN="$remote_run_real"
source "$REMOTE_RUN/scripts/testbox/remote-run-guard.sh"
CARGO_HOME_DIR="$REMOTE_WORKSPACE/cargo-home"
RUSTUP_HOME_DIR="$REMOTE_WORKSPACE/rustup-home"

remove_container() {
  local name="$1"
  if docker inspect "$name" >/dev/null 2>&1; then
    docker rm -f "$name" >/dev/null 2>&1
  else
    docker info >/dev/null 2>&1
  fi
}
cleanup() {
  local status="${1:-$?}"
  set +e
  local cleanup_ok=1
  if [ -n "${CONTAINER_NAME:-}" ]; then
    remove_container "$CONTAINER_NAME" || cleanup_ok=0
  fi
  if [ -n "${OPENRC_CONTAINER_NAME:-}" ]; then
    remove_container "$OPENRC_CONTAINER_NAME" || cleanup_ok=0
  fi
  if [ "$cleanup_ok" = "1" ] && [ -n "${REMOTE_RUN:-}" ]; then
    if assert_remote_run && remove_remote_run >/dev/null 2>&1; then
      :
    else
      cleanup_ok=0
    fi
  fi
  if [ "$cleanup_ok" != "1" ] && [ "$status" -eq 0 ]; then
    status=1
  fi
  return "$status"
}
on_signal() {
  local status="$1"
  trap - EXIT INT TERM
  cleanup
  exit "$status"
}
trap - EXIT INT TERM
on_exit() {
  local status=$?
  trap - EXIT
  cleanup "$status"
  exit $?
}
trap on_exit EXIT
trap 'on_signal 130' INT
trap 'on_signal 143' TERM

mkdir -p "$CARGO_HOME_DIR" "$RUSTUP_HOME_DIR"

# This is a real Alpine/OpenDoas boundary check. It only runs the fixed readiness
# helper, never rc-service or the upgrade runner.
docker run --rm \
  --name "$OPENRC_CONTAINER_NAME" \
  --label "codex.scope=web-local-upgrade" \
  --label "codex.remote_run=$REMOTE_RUN" \
  --cap-drop=ALL \
  --cap-add=CHOWN \
  --cap-add=DAC_OVERRIDE \
  --cap-add=FOWNER \
  --cap-add=SETGID \
  --cap-add=SETUID \
  -v "$REMOTE_RUN:/workspace:ro" \
  "$ALPINE_IMAGE" \
  sh -ec '
    set -euo pipefail
    apk add --no-cache doas
    adduser -D -s /bin/sh xp

    install -Dm0755 /workspace/docs/ops/openrc/xp-upgrade /etc/init.d/xp-upgrade
    install -Dm0755 /workspace/docs/ops/openrc/xp-upgrade-trigger /usr/local/libexec/xp-openrc-upgrade-trigger
    install -Dm0600 /workspace/docs/ops/openrc/doas-xp-upgrade.conf /etc/doas.conf
    chown root:root /etc/doas.conf /etc/init.d/xp-upgrade /usr/local/libexec/xp-openrc-upgrade-trigger

    cat > /sbin/rc-service <<'"'"'EOF'"'"'
#!/bin/sh
touch /tmp/rc-service-called
EOF
    chmod 0755 /sbin/rc-service

    su -s /bin/sh xp -c "test ! -r /etc/doas.conf"
    su -s /bin/sh xp -c "doas -n /usr/local/libexec/xp-openrc-upgrade-trigger --check"
    test ! -e /tmp/rc-service-called

    if /usr/local/libexec/xp-openrc-upgrade-trigger unexpected >/dev/null 2>&1; then
      echo "OpenRC readiness helper accepted an unexpected argument" >&2
      exit 1
    fi
    if su -s /bin/sh xp -c "doas -n /usr/local/libexec/xp-openrc-upgrade-trigger"; then
      echo "OpenRC doas policy allowed helper without --check" >&2
      exit 1
    fi

    sed -i "\\|permit nopass xp as root cmd /sbin/rc-service args xp-upgrade start|d" /etc/doas.conf
    if su -s /bin/sh xp -c "doas -n /usr/local/libexec/xp-openrc-upgrade-trigger --check"; then
      echo "OpenRC readiness check passed after the fixed start rule was removed" >&2
      exit 1
    fi

    rm /usr/local/libexec/xp-openrc-upgrade-trigger
    if su -s /bin/sh xp -c "doas -n /usr/local/libexec/xp-openrc-upgrade-trigger --check"; then
      echo "OpenRC readiness check passed after the helper was removed" >&2
      exit 1
    fi
  '

docker run --rm \
  --name "$CONTAINER_NAME" \
  --label "codex.scope=web-local-upgrade" \
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
  -e CARGO_TARGET_DIR=/workspace/target \
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
    mkdir -p "$TMPDIR"
    timeout 300 rustc --version
    timeout 300 cargo --version

    mkdir -p web/dist
    printf "%s\n" "<!doctype html><title>xp testbox upgrade e2e</title>" > web/dist/index.html

    for attempt in 1 2 3; do
      if cargo fetch --locked; then
        break
      fi
      if [ "$attempt" = "3" ]; then
        exit 1
      fi
      sleep "$((attempt * 5))"
    done

    cargo test upgrade_job::tests -- --nocapture
    cargo test admin_upgrade --lib -- --nocapture
    cargo test --test xp_ops_upgrade -- --nocapture --test-threads=1
  '
REMOTE
