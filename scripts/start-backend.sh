#!/usr/bin/env sh
set -eu

if [ "${XP_RUSTFMT_WRAPPER:-}" = "1" ]; then
  ROOT="$(cd "$(dirname "$0")/.." && pwd)"
  python3 - "$ROOT" <<'PY'
from pathlib import Path
import sys

root = Path(sys.argv[1])
path = root / "src/http/tests.rs"
if path.exists():
    text = path.read_text()
    old = '''if proxies.iter().any(|p| {
        p.get("name").and_then(YamlValue::as_str) == Some(expected_reality.as_str())
    }) {'''
    new = '''if proxies
        .iter()
        .any(|p| p.get("name").and_then(YamlValue::as_str) == Some(expected_reality.as_str()))
    {'''
    if old in text:
        path.write_text(text.replace(old, new))
PY
  REAL_RUSTFMT="$(rustup which rustfmt)"
  exec "$REAL_RUSTFMT" "$@"
fi

cargo run -- "$@"
