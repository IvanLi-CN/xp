#!/usr/bin/env sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOCAL_BIN="$ROOT/target/debug/xp-ops"

if [ -x "$LOCAL_BIN" ]; then
  exec "$LOCAL_BIN" mihomo redact "$@"
fi

exec cargo run --quiet --bin xp-ops -- mihomo redact "$@"
