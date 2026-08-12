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
  compose down
}
trap cleanup EXIT INT TERM

compose up -d

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

XP_E2E_XRAY_MODE=external \
XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
cargo test --test xray_e2e -- --ignored

XP_E2E_XRAY_MODE=external \
XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
XP_E2E_VLESS_PORT="${XP_E2E_VLESS_PORT}" \
cargo test --test xray_mesh_transport_e2e -- --ignored

XP_E2E_XRAY_MODE=external \
XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
XP_E2E_VLESS_PORT="${XP_E2E_VLESS_PORT}" \
XP_E2E_MIHOMO_BIN="${XP_E2E_MIHOMO_BIN}" \
cargo test --test xray_vless_xhttp_e2e -- --ignored

XP_E2E_XRAY_MODE=external \
XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
cargo test --test shared_quota_xray_e2e -- --ignored
