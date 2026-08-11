#!/usr/bin/env sh
set -eu

VERSION="v1.19.29"
RELEASE_BASE="https://github.com/MetaCubeX/mihomo/releases/download/${VERSION}"

case "$(uname -s):$(uname -m)" in
  Linux:x86_64 | Linux:amd64)
    ASSET="mihomo-linux-amd64-compatible-${VERSION}.gz"
    SHA256="5612e698e96c8b8ad15abc4c0a4f098eba9234354b4f248cb97f2528e215b094"
    ;;
  Linux:aarch64 | Linux:arm64)
    ASSET="mihomo-linux-arm64-${VERSION}.gz"
    SHA256="9a868b5e4e0ad91d9d71e1b41b0cfce78aaba44360c30df74a723f8e3926a86c"
    ;;
  Darwin:x86_64 | Darwin:amd64)
    ASSET="mihomo-darwin-amd64-compatible-${VERSION}.gz"
    SHA256="b43980c9bbcf10911f264662a8be4fdf4c95f4567244d6824c3f5365bab0e7d9"
    ;;
  Darwin:arm64 | Darwin:aarch64)
    ASSET="mihomo-darwin-arm64-${VERSION}.gz"
    SHA256="4dc25df9e899f14161911302a8ee5fc9e202ed9c976fc405bf82c50ff27466ca"
    ;;
  *)
    echo "unsupported Mihomo E2E platform: $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

if [ "$#" -gt 1 ]; then
  echo "usage: $0 [OUTPUT]" >&2
  exit 2
fi

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_DIR="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
OUTPUT="${1:-${CARGO_TARGET_DIR:-$REPO_DIR/target}/e2e-tools/mihomo-${VERSION}}"
ARCHIVE="${OUTPUT}.gz.$$"
STAGED_OUTPUT="${OUTPUT}.tmp.$$"

mkdir -p "$(dirname -- "$OUTPUT")"

if [ -x "$OUTPUT" ] && "$OUTPUT" -v 2>&1 | grep -F "$VERSION" >/dev/null; then
  printf '%s\n' "$OUTPUT"
  exit 0
fi

cleanup() {
  rm -f -- "$ARCHIVE" "$STAGED_OUTPUT"
}
trap cleanup EXIT INT TERM

curl --fail --location --silent --show-error \
  --retry 3 --retry-all-errors --retry-delay 1 \
  --output "$ARCHIVE" \
  "$RELEASE_BASE/$ASSET"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA256="$(sha256sum "$ARCHIVE" | awk '{print $1}')"
else
  ACTUAL_SHA256="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')"
fi

if [ "$ACTUAL_SHA256" != "$SHA256" ]; then
  echo "Mihomo archive checksum mismatch: expected $SHA256, got $ACTUAL_SHA256" >&2
  exit 1
fi

gzip -dc "$ARCHIVE" >"$STAGED_OUTPUT"
chmod 0755 "$STAGED_OUTPUT"
mv -f -- "$STAGED_OUTPUT" "$OUTPUT"

printf '%s\n' "$OUTPUT"
