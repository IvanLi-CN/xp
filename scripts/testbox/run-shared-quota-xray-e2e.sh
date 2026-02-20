#!/usr/bin/env bash
set -euo pipefail

# Run shared-quota real-xray e2e tests on the shared testbox (codex-testbox).
#
# This follows the shared-testbox-runner rules:
# - per-run isolation under /srv/codex/workspaces/$USER
# - unique docker compose project name
# - LXC cap compatibility override
# - safe cleanup (only resources created by this run)

TESTBOX="${TESTBOX:-codex-testbox}"

# 1) Identify local repo root (fallback to current dir if not a git repo).
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

if [ ! -f "$REPO_ROOT/web/dist/index.html" ]; then
  echo "missing $REPO_ROOT/web/dist/index.html; run 'cd web && bun run build' locally" >&2
  exit 2
fi

REPO_NAME="$(basename "$REPO_ROOT")"
PATH_HASH8="$(python3 - "$REPO_ROOT" <<'PY'
import hashlib, os, sys
p=os.path.realpath(sys.argv[1]).encode()
print(hashlib.sha256(p).hexdigest()[:8])
PY
)"

# 2) Per-run identifiers.
GIT_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo nogit)"
RUN_ID="$(date -u +%Y%m%d_%H%M%S)_$GIT_SHA"
WORKSPACE_SLUG="${REPO_NAME}__${PATH_HASH8}"

REMOTE_BASE="/srv/codex/workspaces/$USER"
REMOTE_WORKSPACE="$REMOTE_BASE/$WORKSPACE_SLUG"
REMOTE_RUN="$REMOTE_WORKSPACE/runs/$RUN_ID"

COMPOSE_PROJECT_RAW="codex_${WORKSPACE_SLUG}_${RUN_ID}"
COMPOSE_PROJECT="$(python3 - "$COMPOSE_PROJECT_RAW" <<'PY'
import re, sys
s=sys.argv[1].lower()
s=re.sub(r'[^a-z0-9_-]+','_',s).strip('_')
print(s[:63] if len(s)>63 else s)
PY
)"

echo "testbox=$TESTBOX"
echo "remote_run=$REMOTE_RUN"
echo "compose_project=$COMPOSE_PROJECT"

# 3) Create remote run dir and attach minimal metadata.
CREATED_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
ssh -o BatchMode=yes "$TESTBOX" "mkdir -p '$REMOTE_RUN' && cat > '$REMOTE_WORKSPACE/workspace.txt'" <<TXT
local_repo_root=$REPO_ROOT
created_utc=$CREATED_UTC
TXT

# 4) Sync repo to remote run dir.
rsync -az --delete \
  --exclude '.git/' \
  --exclude 'node_modules/' \
  --exclude 'target/' \
  --exclude 'web/node_modules/' \
  "$REPO_ROOT/" "$TESTBOX:$REMOTE_RUN/"

# 5) Run on testbox.
ssh -o BatchMode=yes "$TESTBOX" \
  "REMOTE_RUN='$REMOTE_RUN' COMPOSE_PROJECT='$COMPOSE_PROJECT' bash -s" <<'REMOTE'
set -euo pipefail

REMOTE_RUN="${REMOTE_RUN:?}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:?}"

cleanup() {
  set +e
  if [ -n "${REMOTE_RUN:-}" ] && [ -d "$REMOTE_RUN/scripts/e2e" ]; then
    cd "$REMOTE_RUN/scripts/e2e" || exit 0
    if [ -f "docker-compose.xray.yml" ] && [ -f ".codex.caps-compat.yaml" ]; then
      docker compose -p "$COMPOSE_PROJECT" -f "docker-compose.xray.yml" -f ".codex.caps-compat.yaml" down -v --remove-orphans >/dev/null 2>&1 || true
    fi
  fi
  if [ -n "${REMOTE_RUN:-}" ]; then
    rm -rf "$REMOTE_RUN" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

cd "$REMOTE_RUN/scripts/e2e"

# Random host ports (avoid collisions).
XP_E2E_XRAY_API_PORT="$(python3 - <<'PY'
import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()
PY
)"
XP_E2E_SS_PORT="$(python3 - <<'PY'
import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()
PY
)"
while [ "$XP_E2E_SS_PORT" = "$XP_E2E_XRAY_API_PORT" ]; do
  XP_E2E_SS_PORT="$(python3 - <<'PY'
import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()
PY
  )"
done
export XP_E2E_XRAY_API_PORT XP_E2E_SS_PORT

COMPOSE_FILE="docker-compose.xray.yml"

# LXC quirk: CAP_SETFCAP is not available. Default Docker caps include it.
# Workaround: drop ALL caps, then add back a known-good set (default minus SETFCAP).
caps_override=".codex.caps-compat.yaml"
services="$(docker compose -f "$COMPOSE_FILE" config --services)"
{
  echo "services:"
  for s in $services; do
    cat <<YAML
  $s:
    cap_drop:
      - ALL
    cap_add:
      - CHOWN
      - DAC_OVERRIDE
      - FSETID
      - FOWNER
      - MKNOD
      - NET_RAW
      - SETGID
      - SETUID
      - SETPCAP
      - NET_BIND_SERVICE
      - SYS_CHROOT
      - KILL
      - AUDIT_WRITE
YAML
  done
} > "$caps_override"

echo "starting xray: api_port=$XP_E2E_XRAY_API_PORT ss_port=$XP_E2E_SS_PORT"
docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" -f "$caps_override" up -d

echo "waiting for xray gRPC on 127.0.0.1:$XP_E2E_XRAY_API_PORT..."
python3 - <<'PY'
import socket, time, os, sys
host="127.0.0.1"
port=int(os.environ["XP_E2E_XRAY_API_PORT"])
deadline=time.time()+10
while time.time()<deadline:
  s=socket.socket(); s.settimeout(0.2)
  try:
    s.connect((host, port))
    sys.exit(0)
  except OSError:
    time.sleep(0.1)
  finally:
    s.close()
print("xray did not become ready in time", file=sys.stderr)
sys.exit(1)
PY

cd "$REMOTE_RUN"

export RUST_TEST_THREADS=1
export XP_E2E_XRAY_MODE=external
export XP_E2E_XRAY_API_ADDR="127.0.0.1:$XP_E2E_XRAY_API_PORT"

cargo test --test shared_quota_xray_e2e -- --ignored
REMOTE

echo "OK: shared_quota_xray_e2e on $TESTBOX"

