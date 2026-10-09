#!/usr/bin/env sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
XP_E2E_COMPOSE_PROJECT="${XP_E2E_COMPOSE_PROJECT:-xp-e2e}"
export XP_E2E_COMPOSE_PROJECT
XP_E2E_COMPOSE_OVERRIDE_FILE="${XP_E2E_COMPOSE_OVERRIDE_FILE:-}"
export XP_E2E_COMPOSE_OVERRIDE_FILE

if [ -z "${XP_E2E_MIHOMO_BIN:-}" ]; then
  XP_E2E_MIHOMO_BIN="$($SCRIPT_DIR/install-mihomo-v1.19.29.sh)"
fi
export XP_E2E_MIHOMO_BIN

compose() {
  if docker compose version >/dev/null 2>&1; then
    if [ -n "$XP_E2E_COMPOSE_OVERRIDE_FILE" ]; then
      docker compose -p "$XP_E2E_COMPOSE_PROJECT" -f "$SCRIPT_DIR/docker-compose.xray.yml" -f "$XP_E2E_COMPOSE_OVERRIDE_FILE" "$@"
    else
      docker compose -p "$XP_E2E_COMPOSE_PROJECT" -f "$SCRIPT_DIR/docker-compose.xray.yml" "$@"
    fi
  else
    if [ -n "$XP_E2E_COMPOSE_OVERRIDE_FILE" ]; then
      docker-compose -p "$XP_E2E_COMPOSE_PROJECT" -f "$SCRIPT_DIR/docker-compose.xray.yml" -f "$XP_E2E_COMPOSE_OVERRIDE_FILE" "$@"
    else
      docker-compose -p "$XP_E2E_COMPOSE_PROJECT" -f "$SCRIPT_DIR/docker-compose.xray.yml" "$@"
    fi
  fi
}

if [ -z "${XP_E2E_XRAY_API_PORT:-}" ]; then
  XP_E2E_XRAY_API_PORT="$(
    python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
  )"
fi
export XP_E2E_XRAY_API_PORT

if [ -z "${XP_E2E_SS_PORT:-}" ]; then
  XP_E2E_SS_PORT="$(
    python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
  )"
fi
while [ "${XP_E2E_SS_PORT}" = "${XP_E2E_XRAY_API_PORT}" ]; do
  XP_E2E_SS_PORT="$(
    python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
  )"
done
export XP_E2E_SS_PORT

if [ -z "${XP_E2E_VLESS_PORT:-}" ]; then
  XP_E2E_VLESS_PORT="$(
    python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
  )"
fi
while [ "${XP_E2E_VLESS_PORT}" = "${XP_E2E_XRAY_API_PORT}" ] ||
  [ "${XP_E2E_VLESS_PORT}" = "${XP_E2E_SS_PORT}" ]; do
  XP_E2E_VLESS_PORT="$(
    python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
  )"
done
export XP_E2E_VLESS_PORT

cleanup() {
  if [ "${XP_E2E_COMPOSE_STARTED:-0}" -eq 1 ]; then
    compose down
  fi
  if [ -n "${XP_E2E_BINARY_DIR:-}" ]; then
    rm -rf "$XP_E2E_BINARY_DIR"
  fi
}
trap cleanup EXIT INT TERM

XP_E2E_BINARY_DIR="$(mktemp -d "${TMPDIR:-/tmp}/xp-xray-e2e-binaries.XXXXXX")"
XP_E2E_COMPOSE_STARTED=0

echo "building Xray E2E test binaries once..."
XP_E2E_MANIFEST="$XP_E2E_BINARY_DIR/cargo.json"
XP_E2E_BUILD_LOG="$XP_E2E_BINARY_DIR/cargo-build.log"
cargo test \
  --locked \
  --test xray_e2e \
  --test xray_mesh_transport_e2e \
  --test xray_vless_xhttp_e2e \
  --test shared_quota_xray_e2e \
  --no-run \
  --message-format=json >"$XP_E2E_MANIFEST" 2>"$XP_E2E_BUILD_LOG" || {
  cat "$XP_E2E_BUILD_LOG" >&2
  exit 1
}

python3 - "$XP_E2E_MANIFEST" "$XP_E2E_BINARY_DIR" <<'PY'
import json
import pathlib
import sys

manifest = pathlib.Path(sys.argv[1])
output_dir = pathlib.Path(sys.argv[2])
wanted = {
    "xray_e2e",
    "xray_mesh_transport_e2e",
    "xray_vless_xhttp_e2e",
    "shared_quota_xray_e2e",
}
paths = {}
for line in manifest.read_text().splitlines():
    try:
        record = json.loads(line)
    except json.JSONDecodeError:
        continue
    if record.get("reason") != "compiler-artifact":
        continue
    target = record.get("target", {})
    name = target.get("name")
    executable = record.get("executable")
    if name in wanted and "test" in target.get("kind", []) and executable:
        paths[name] = executable

missing = sorted(wanted - paths.keys())
if missing:
    raise SystemExit(f"missing compiled Xray E2E test binaries: {', '.join(missing)}")
for name, executable in paths.items():
    (output_dir / name).write_text(executable)
PY

XRAY_E2E_BIN="$(cat "$XP_E2E_BINARY_DIR/xray_e2e")"
XRAY_MESH_BIN="$(cat "$XP_E2E_BINARY_DIR/xray_mesh_transport_e2e")"
XRAY_VLESS_BIN="$(cat "$XP_E2E_BINARY_DIR/xray_vless_xhttp_e2e")"
SHARED_QUOTA_BIN="$(cat "$XP_E2E_BINARY_DIR/shared_quota_xray_e2e")"

run_suite() {
  suite="$1"
  shift
  started_at="$(date +%s)"
  echo "running ${suite}..."
  "$@"
  finished_at="$(date +%s)"
  echo "completed ${suite} in $((finished_at - started_at))s"
}

compose up -d
XP_E2E_COMPOSE_STARTED=1

port_open() {
  python3 - "$XP_E2E_XRAY_API_PORT" <<'PY'
import socket
import sys

host = "127.0.0.1"
port = int(sys.argv[1])
s = socket.socket()
s.settimeout(0.1)
try:
    s.connect((host, port))
except OSError:
    sys.exit(1)
else:
    sys.exit(0)
finally:
    s.close()
PY
}

echo "waiting for xray gRPC on 127.0.0.1:${XP_E2E_XRAY_API_PORT}..."
i=0
while ! port_open >/dev/null 2>&1; do
  i=$((i + 1))
  if [ "$i" -gt 100 ]; then
    echo "xray did not become ready in time"
    compose logs --no-color xray || true
    exit 1
  fi
  sleep 0.1
done

# These ignored suites share one external Xray instance and forwarded ports.
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}"

run_suite xray_e2e env \
  XP_E2E_XRAY_MODE=external \
  XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
  "$XRAY_E2E_BIN" --ignored

run_suite mesh-reality-fallback env \
  XP_E2E_XRAY_MODE=external \
  XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
  XP_E2E_VLESS_PORT="${XP_E2E_VLESS_PORT}" \
  "$XRAY_MESH_BIN" --ignored --exact \
  reality_fallback_reuses_one_h2_connection_and_recovers_after_disconnect \
  --test-threads=1

run_suite mesh-xhttp-fallback env \
  XP_E2E_XRAY_MODE=external \
  XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
  XP_E2E_VLESS_PORT="${XP_E2E_VLESS_PORT}" \
  "$XRAY_MESH_BIN" --ignored --exact \
  xhttp_endpoint_reality_fallback_reuses_one_h2_connection_and_recovers_after_disconnect \
  --test-threads=1

run_suite vless-xhttp env \
  XP_E2E_XRAY_MODE=external \
  XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
  XP_E2E_VLESS_PORT="${XP_E2E_VLESS_PORT}" \
  XP_E2E_MIHOMO_BIN="${XP_E2E_MIHOMO_BIN}" \
  "$XRAY_VLESS_BIN" --ignored

run_suite shared-quota env \
  XP_E2E_XRAY_MODE=external \
  XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
  "$SHARED_QUOTA_BIN" --ignored
