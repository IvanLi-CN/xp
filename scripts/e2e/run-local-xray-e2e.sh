#!/usr/bin/env sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

compose() {
  if docker compose version >/dev/null 2>&1; then
    docker compose -f "$SCRIPT_DIR/docker-compose.xray.yml" "$@"
  else
    docker-compose -f "$SCRIPT_DIR/docker-compose.xray.yml" "$@"
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

XP_E2E_XRAY_MODE=external \
XP_E2E_XRAY_API_ADDR="127.0.0.1:${XP_E2E_XRAY_API_PORT}" \
cargo test --test xray_e2e -- --ignored
