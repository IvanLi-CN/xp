#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$REPO_ROOT/web/dist"
FINGERPRINT="$REPO_ROOT/web/.xp-dist-fingerprint"

[[ -d "$DIST_DIR" ]] || {
  printf 'web dist directory is missing: %s\n' "$DIST_DIR" >&2
  exit 1
}

if command -v sha256sum >/dev/null 2>&1; then
  hash_file() { sha256sum "$1" | awk '{print $1}'; }
  hash_stream() { sha256sum | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  hash_file() { shasum -a 256 "$1" | awk '{print $1}'; }
  hash_stream() { shasum -a 256 | awk '{print $1}'; }
else
  printf 'sha256sum or shasum is required\n' >&2
  exit 1
fi

tmp_file="$(mktemp "$REPO_ROOT/web/.xp-dist-fingerprint.XXXXXX")"
trap 'rm -f "$tmp_file"' EXIT

(
  cd "$DIST_DIR"
  while IFS= read -r -d '' file; do
    relative="${file#./}"
    printf '%s\0%s\0' "$relative" "$(hash_file "$file")"
  done < <(find . -type f -print0 | LC_ALL=C sort -z)
) | hash_stream > "$tmp_file"

if [[ ! -f "$FINGERPRINT" ]] || ! cmp -s "$tmp_file" "$FINGERPRINT"; then
  mv "$tmp_file" "$FINGERPRINT"
fi
