#!/usr/bin/env bash
set -euo pipefail

# Run real-xray e2e tests on the shared testbox (codex-testbox).
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
REMOTE_SUBNET_CLAIMS="$REMOTE_BASE/.shared-testbox-subnet-claims"

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
  "REMOTE_RUN='$REMOTE_RUN' COMPOSE_PROJECT='$COMPOSE_PROJECT' SUBNET_CLAIM_ROOT='$REMOTE_SUBNET_CLAIMS' bash -s" <<'REMOTE'
set -euo pipefail

REMOTE_RUN="${REMOTE_RUN:?}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:?}"
SUBNET_CLAIM_ROOT="${SUBNET_CLAIM_ROOT:?}"

cleanup() {
  set +e
  if [ -n "${REMOTE_RUN:-}" ] && [ -d "$REMOTE_RUN/scripts/e2e" ]; then
    cd "$REMOTE_RUN/scripts/e2e" || exit 0
    if [ -f "docker-compose.xray.yml" ] && [ -f ".codex.caps-compat.yaml" ] && [ -f ".codex.net-compat.yaml" ]; then
      docker compose -p "$COMPOSE_PROJECT" -f "docker-compose.xray.yml" -f ".codex.caps-compat.yaml" -f ".codex.net-compat.yaml" down -v --remove-orphans >/dev/null 2>&1 || true
    fi
  fi
  if [ -n "${SUBNET_CLAIM_DIR:-}" ] && [ -d "$SUBNET_CLAIM_DIR" ]; then
    rm -rf "$SUBNET_CLAIM_DIR" >/dev/null 2>&1 || true
  fi
  if [ -n "${REMOTE_RUN:-}" ]; then
    rm -rf "$REMOTE_RUN" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

cd "$REMOTE_RUN/scripts/e2e"

if ! command -v make >/dev/null 2>&1; then
  echo "missing 'make' on codex-testbox; vendored OpenSSL builds require it" >&2
  echo "repair with shared-testbox-bootstrap before running real-Xray suites" >&2
  exit 2
fi

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
SUBNET_CLAIM_DIR=""

# LXC quirk: CAP_SETFCAP is not available. Default Docker caps include it.
# Workaround: drop ALL caps, then add back a known-good set (default minus SETFCAP).
caps_override=".codex.caps-compat.yaml"
net_override=".codex.net-compat.yaml"
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

mapfile -t subnet_claim_info < <(
  python3 - "$SUBNET_CLAIM_ROOT" "$REMOTE_RUN" "$COMPOSE_PROJECT" <<'PY'
import ipaddress
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

claim_root = pathlib.Path(sys.argv[1])
remote_run = pathlib.Path(sys.argv[2])
compose_project = sys.argv[3]
lock_dir = claim_root / ".allocator.lock"
lock_timeout_seconds = 30
lock_poll_seconds = 0.2

claim_root.mkdir(parents=True, exist_ok=True)

deadline = time.time() + lock_timeout_seconds
while True:
    try:
        os.mkdir(lock_dir)
        break
    except FileExistsError:
        if time.time() >= deadline:
            print("timed out waiting for shared-testbox subnet allocator lock", file=sys.stderr)
            sys.exit(1)
        time.sleep(lock_poll_seconds)

used = []
claimed = []

try:
    try:
        docker_ids = subprocess.check_output(
            ["docker", "network", "ls", "-q"],
            text=True,
        ).split()
    except subprocess.CalledProcessError:
        docker_ids = []

    if docker_ids:
        inspect = json.loads(
            subprocess.check_output(["docker", "network", "inspect", *docker_ids], text=True)
        )
        for network in inspect:
            for cfg in (network.get("IPAM") or {}).get("Config") or []:
                subnet = cfg.get("Subnet")
                if subnet:
                    try:
                        used.append(ipaddress.ip_network(subnet, strict=False))
                    except ValueError:
                        pass

    for line in subprocess.check_output(
        ["ip", "-o", "-4", "addr", "show"],
        text=True,
    ).splitlines():
        parts = line.split()
        if len(parts) >= 4:
            try:
                used.append(ipaddress.ip_network(parts[3], strict=False))
            except ValueError:
                pass

    for claim_dir in claim_root.iterdir():
        if not claim_dir.is_dir() or claim_dir.name.startswith("."):
            continue

        run_path_file = claim_dir / "run_path"
        subnet_file = claim_dir / "subnet"
        if not run_path_file.exists() or not subnet_file.exists():
            shutil.rmtree(claim_dir, ignore_errors=True)
            continue

        run_path = pathlib.Path(run_path_file.read_text().strip())
        if not run_path.exists():
            shutil.rmtree(claim_dir, ignore_errors=True)
            continue

        try:
            claimed.append(ipaddress.ip_network(subnet_file.read_text().strip(), strict=False))
        except ValueError:
            shutil.rmtree(claim_dir, ignore_errors=True)

    for octet in range(0, 256):
        candidate = ipaddress.ip_network(f"10.203.{octet}.0/24")
        if any(candidate.overlaps(existing) for existing in used):
            continue
        if any(candidate.overlaps(existing) for existing in claimed):
            continue

        claim_name = f"{candidate.network_address.exploded.replace('.', '_')}_{candidate.prefixlen}"
        claim_dir = claim_root / claim_name
        if claim_dir.exists():
            continue

        claim_dir.mkdir()
        (claim_dir / "run_path").write_text(f"{remote_run}\n")
        (claim_dir / "compose_project").write_text(f"{compose_project}\n")
        (claim_dir / "subnet").write_text(f"{candidate}\n")
        print(candidate)
        print(claim_dir)
        sys.exit(0)

    print("failed to find free subnet for shared testbox compose run", file=sys.stderr)
    sys.exit(1)
finally:
    try:
        os.rmdir(lock_dir)
    except FileNotFoundError:
        pass
PY
)

TESTBOX_SUBNET="${subnet_claim_info[0]:-}"
SUBNET_CLAIM_DIR="${subnet_claim_info[1]:-}"
if [ -z "$TESTBOX_SUBNET" ] || [ -z "$SUBNET_CLAIM_DIR" ]; then
  echo "failed to allocate isolated shared-testbox subnet claim" >&2
  exit 1
fi

cat > "$net_override" <<YAML
networks:
  default:
    ipam:
      config:
        - subnet: ${TESTBOX_SUBNET}
YAML

echo "selected subnet: $TESTBOX_SUBNET"
echo "starting xray: api_port=$XP_E2E_XRAY_API_PORT ss_port=$XP_E2E_SS_PORT"
docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" -f "$caps_override" -f "$net_override" up -d

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

cargo test --test xray_e2e -- --ignored
cargo test --test shared_quota_xray_e2e -- --ignored
REMOTE

echo "OK: xray_e2e + shared_quota_xray_e2e on $TESTBOX"
