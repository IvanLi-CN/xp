#!/usr/bin/env bash
set -euo pipefail

readonly version="v1.19.29"
readonly asset="mihomo-linux-amd64-v1.19.29.gz"
readonly expected_sha256="60de76a35a6cbf7b4fa4a20f5c257c24345d1d635ab1aa3877022a1997ef413c"
readonly fixture="tests/fixtures/mihomo-smux-v1.19.29.yaml"
readonly url="https://github.com/MetaCubeX/mihomo/releases/download/${version}/${asset}"

if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
  printf '%s\n' "Mihomo fixture validation requires Linux x86_64." >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

curl --fail --location --retry 3 --retry-all-errors --silent --show-error "$url" \
  --output "$tmp_dir/$asset"
actual_sha256="$(sha256sum "$tmp_dir/$asset" | awk '{print $1}')"
if [ "$actual_sha256" != "$expected_sha256" ]; then
  printf '%s\n' "Mihomo fixture checksum mismatch." >&2
  exit 1
fi

gzip --decompress --stdout "$tmp_dir/$asset" >"$tmp_dir/mihomo"
chmod 0755 "$tmp_dir/mihomo"
"$tmp_dir/mihomo" -t -f "$fixture"
