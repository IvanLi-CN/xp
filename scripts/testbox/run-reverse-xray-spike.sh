#!/usr/bin/env bash
set -euo pipefail

# Fixed-Xray Reality Mesh Reverse spike. All Docker state is scoped to one remote run.
TESTBOX="${TESTBOX:-codex-testbox}"
REPO_ROOT="$(git rev-parse --show-toplevel)"
REPO_ROOT="$(python3 - "$REPO_ROOT" <<'PY'
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
)"
REPO_NAME="$(basename "$REPO_ROOT")"
PATH_HASH8="$(python3 - "$REPO_ROOT" <<'PY'
import hashlib, os, sys
print(hashlib.sha256(os.path.realpath(sys.argv[1]).encode()).hexdigest()[:8])
PY
)"
GIT_SHA="$(git rev-parse --short HEAD)"
RUN_ID="$(date -u +%Y%m%d_%H%M%S)_${GIT_SHA}_reverse"
REMOTE_BASE="/srv/codex/workspaces/$USER"
REMOTE_WORKSPACE="$REMOTE_BASE/${REPO_NAME}__${PATH_HASH8}"
REMOTE_RUN="$REMOTE_WORKSPACE/runs/$RUN_ID"
COMPOSE_PROJECT="codex_${REPO_NAME}_${PATH_HASH8}_${RUN_ID}"
COMPOSE_PROJECT="$(python3 - "$COMPOSE_PROJECT" <<'PY'
import re, sys
print(re.sub(r'[^a-z0-9_-]+', '_', sys.argv[1].lower()).strip('_')[:63])
PY
)"

cleanup() {
  local status=$?
  set +e
  if [[ "$status" != 0 ]]; then
    ssh -o BatchMode=yes "$TESTBOX" "cd '$REMOTE_RUN' && docker compose -p '$COMPOSE_PROJECT' -f scripts/testbox/reverse-xray-spike-compose.yml logs --no-color rendezvous target" >&2
  fi
  ssh -o BatchMode=yes "$TESTBOX" "cd '$REMOTE_RUN' && docker compose -p '$COMPOSE_PROJECT' -f scripts/testbox/reverse-xray-spike-compose.yml -f .codex.caps-compat.yml down -v --remove-orphans; rm -rf '$REMOTE_RUN'" >/dev/null 2>&1
}
trap cleanup EXIT INT TERM

ssh -o BatchMode=yes "$TESTBOX" "mkdir -p '$REMOTE_RUN'"
rsync -az --delete --exclude '.git/' --exclude 'target/' --exclude 'web/node_modules/' \
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
  cargo test --test reverse_xray_spike -- --ignored"
echo "reverse-xray-spike=pass xray=26.3.27 expected_commit=d2758a023cd7f4174a5a5fa4ff66e487d4342ba0"
