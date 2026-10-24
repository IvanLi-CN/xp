#!/usr/bin/env bash
set -euo pipefail

# Fixed-Xray Reality Mesh Reverse spike. All Docker state is scoped to one remote run.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=runner-env.sh
source "$SCRIPT_DIR/runner-env.sh"
REPO_ROOT="$(git rev-parse --show-toplevel)"
REPO_ROOT="$(python3 - "$REPO_ROOT" <<'PY'
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
)"
REPO_NAME="$(basename "$REPO_ROOT")"
testbox_validate_component repository "$REPO_NAME"
if ! git diff --quiet HEAD -- || ! git diff --cached --quiet || [[ -n "$(git ls-files --others --exclude-standard)" ]]; then
  echo "reverse-xray-spike requires a clean worktree" >&2
  exit 1
fi
PATH_HASH8="$(python3 - "$REPO_ROOT" <<'PY'
import hashlib, os, sys
print(hashlib.sha256(os.path.realpath(sys.argv[1]).encode()).hexdigest()[:8])
PY
)"
GIT_SHA="$(git rev-parse HEAD)"
GIT_SHA_SHORT="$(git rev-parse --short HEAD)"
RUN_NONCE="$(od -An -N4 -tu4 /dev/urandom | tr -d ' ')"
RUN_ID="$(date -u +%Y%m%d_%H%M%S)_${GIT_SHA_SHORT}_${RUN_NONCE}_reverse"
REMOTE_BASE="$TESTBOX_REMOTE_ROOT/workspaces/$USER"
REMOTE_WORKSPACE="$REMOTE_BASE/${REPO_NAME}__${PATH_HASH8}"
REMOTE_RUN="$REMOTE_WORKSPACE/runs/$RUN_ID"
COMPOSE_PROJECT="codex_${REPO_NAME}_${PATH_HASH8}_${RUN_ID}"
COMPOSE_PROJECT="$(testbox_bounded_name "$COMPOSE_PROJECT")"
REMOTE_RUN_CREATED=0
cleanup() {
  local status="${1:-$?}"
  set +e
  local cleanup_ok=1
  if [[ "$REMOTE_RUN_CREATED" = "1" && "$status" != 0 ]] && testbox_remote_run_verify "$REMOTE_RUN" "$RUN_ID"; then
    ssh -o BatchMode=yes "$TESTBOX" "test \"\$(cat -- '$REMOTE_RUN/.codex-run-id')\" = '$RUN_ID' && cd '$REMOTE_RUN' && docker compose -p '$COMPOSE_PROJECT' -f scripts/testbox/reverse-xray-spike-compose.yml logs --no-color rendezvous target" >&2
  fi
  if [[ "$REMOTE_RUN_CREATED" = "1" ]]; then
    if ssh -o BatchMode=yes "$TESTBOX" \
      "TESTBOX_REMOTE_ROOT_B64=\$(printf '%s' '$TESTBOX_REMOTE_ROOT' | base64 | tr -d '\\n') REMOTE_RUN_B64=\$(printf '%s' '$REMOTE_RUN' | base64 | tr -d '\\n') COMPOSE_PROJECT_B64=\$(printf '%s' '$COMPOSE_PROJECT' | base64 | tr -d '\\n') RUN_ID_B64=\$(printf '%s' '$RUN_ID' | base64 | tr -d '\\n') bash -s" <<'REMOTE_CLEANUP' >/dev/null 2>&1
set -euo pipefail
TESTBOX_REMOTE_ROOT="$(printf '%s' "${TESTBOX_REMOTE_ROOT_B64:?}" | base64 -d)"
REMOTE_RUN="$(printf '%s' "${REMOTE_RUN_B64:?}" | base64 -d)"
COMPOSE_PROJECT="$(printf '%s' "${COMPOSE_PROJECT_B64:?}" | base64 -d)"
RUN_ID="$(printf '%s' "${RUN_ID_B64:?}" | base64 -d)"
GUARD_SCRIPT="$REMOTE_RUN/scripts/testbox/remote-run-guard.py"
if [ -f "$GUARD_SCRIPT" ]; then
  RUN_ID="$RUN_ID" python3 "$GUARD_SCRIPT" verify "$TESTBOX_REMOTE_ROOT" "$REMOTE_RUN"
  cd "$REMOTE_RUN"
  docker compose -p "$COMPOSE_PROJECT" -f scripts/testbox/reverse-xray-spike-compose.yml -f .codex.caps-compat.yml down -v --remove-orphans
fi
REMOTE_CLEANUP
    then
      :
    else
      cleanup_ok=0
    fi
    if [ "$cleanup_ok" = "1" ]; then
      testbox_remote_cleanup "$REMOTE_RUN" "$RUN_ID" >/dev/null 2>&1 || cleanup_ok=0
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
  cleanup "$status"
  exit "$status"
}
on_exit() {
  local status=$?
  trap - EXIT
  cleanup "$status"
  local cleanup_status=$?
  if [ "$status" -eq 0 ] && [ "$cleanup_status" -ne 0 ]; then
    status="$cleanup_status"
  fi
  exit "$status"
}
trap on_exit EXIT
trap 'on_signal 130' INT
trap 'on_signal 143' TERM
testbox_remote_root_preflight
REMOTE_RUN_CREATED=1
REMOTE_RUN="$(testbox_remote_run_create "$REMOTE_RUN")"
REMOTE_WORKSPACE="$(dirname "$(dirname "$REMOTE_RUN")")"

rsync -az --no-perms --delete --exclude '.codex-run-id' --exclude '.git/' --exclude 'target/' --exclude 'web/node_modules/' \
  "$REPO_ROOT/" "$TESTBOX:$REMOTE_RUN/"
ssh -o BatchMode=yes "$TESTBOX" "cd '$REMOTE_RUN' && \
  REVERSE_SPIKE_RVS_API_PORT=\$(python3 - <<'PY'
import socket
s=socket.socket(); s.bind(('127.0.0.1', 0)); print(s.getsockname()[1]); s.close()
PY
) && \
  REVERSE_SPIKE_TARGET_API_PORT=\$(python3 - <<'PY'
import socket
s=socket.socket(); s.bind(('127.0.0.1', 0)); print(s.getsockname()[1]); s.close()
PY
) && \
  REVERSE_SPIKE_PORTAL_PORT=\$(python3 - <<'PY'
import socket
s=socket.socket(); s.bind(('127.0.0.1', 0)); print(s.getsockname()[1]); s.close()
PY
) && export REVERSE_SPIKE_RVS_API_PORT REVERSE_SPIKE_TARGET_API_PORT REVERSE_SPIKE_PORTAL_PORT && \
  services=\$(docker compose -f scripts/testbox/reverse-xray-spike-compose.yml config --services); \
  { echo services:; for service in \$services; do printf '  %s:\\n    cap_drop: [ALL]\\n    cap_add: [AUDIT_WRITE, CHOWN, DAC_OVERRIDE, FOWNER, FSETID, KILL, MKNOD, NET_BIND_SERVICE, NET_RAW, SETGID, SETPCAP, SETUID, SYS_CHROOT]\\n' \"\$service\"; done; } > .codex.caps-compat.yml && \
  docker compose -p '$COMPOSE_PROJECT' -f scripts/testbox/reverse-xray-spike-compose.yml -f .codex.caps-compat.yml up -d && \
  docker compose -p '$COMPOSE_PROJECT' -f scripts/testbox/reverse-xray-spike-compose.yml -f .codex.caps-compat.yml exec -T rendezvous xray version | tee .reverse-xray-version && \
  grep -q '26.3.27' .reverse-xray-version && \
  docker image inspect ghcr.io/xtls/xray-core@sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7 --format '{{index .RepoDigests 0}}' | grep -q 'sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7' && \
  docker compose -p '$COMPOSE_PROJECT' -f scripts/testbox/reverse-xray-spike-compose.yml -f .codex.caps-compat.yml exec -T rendezvous xray run -test -c /etc/xray/rendezvous.json && \
  docker compose -p '$COMPOSE_PROJECT' -f scripts/testbox/reverse-xray-spike-compose.yml -f .codex.caps-compat.yml exec -T target xray run -test -c /etc/xray/target.json"

ssh -o BatchMode=yes "$TESTBOX" "cd '$REMOTE_RUN' && \
  REVERSE_SPIKE_RVS_API_PORT=\$(docker port \"$COMPOSE_PROJECT-rendezvous-1\" 10085/tcp | sed -E 's/.*://') && \
  REVERSE_SPIKE_TARGET_API_PORT=\$(docker port \"$COMPOSE_PROJECT-target-1\" 10085/tcp | sed -E 's/.*://') && \
  REVERSE_SPIKE_PORTAL_PORT=\$(docker port \"$COMPOSE_PROJECT-rendezvous-1\" 10086/tcp | sed -E 's/.*://') && \
  XP_REVERSE_XRAY_RVS_API_ADDR=127.0.0.1:\$REVERSE_SPIKE_RVS_API_PORT \
  XP_REVERSE_XRAY_TARGET_API_ADDR=127.0.0.1:\$REVERSE_SPIKE_TARGET_API_PORT \
  XP_REVERSE_XRAY_SOCKS_ADDR=127.0.0.1:\$REVERSE_SPIKE_PORTAL_PORT \
  XP_REVERSE_XRAY_TARGET_CONTAINER=\$(docker compose -p '$COMPOSE_PROJECT' -f scripts/testbox/reverse-xray-spike-compose.yml ps -q target) \
  cargo test --test reverse_xray_spike -- --ignored"
echo "reverse-xray-spike=pass xray=26.3.27 tested_commit=$GIT_SHA"
