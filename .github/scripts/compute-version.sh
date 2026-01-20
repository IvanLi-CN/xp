#!/usr/bin/env bash
set -euo pipefail

git fetch --tags --force

base_version="$(
  awk '
    $0 ~ /^\[package\]/ { in_pkg=1; next }
    $0 ~ /^\[/ { in_pkg=0 }
    in_pkg && $1 == "version" {
      gsub(/"/, "", $3);
      print $3;
      exit
    }
  ' Cargo.toml
)"

if [[ -z "${base_version}" ]]; then
  echo "failed to parse [package].version from Cargo.toml" >&2
  exit 1
fi

IFS='.' read -r major minor patch <<<"${base_version}"
if [[ -z "${major}" || -z "${minor}" || -z "${patch}" ]]; then
  echo "invalid base version: ${base_version}" >&2
  exit 1
fi

candidate="${patch}"
while git rev-parse -q --verify "refs/tags/v${major}.${minor}.${candidate}" >/dev/null; do
  candidate="$((candidate + 1))"
done

effective_version="${major}.${minor}.${candidate}"

echo "XP_EFFECTIVE_VERSION=${effective_version}"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  echo "version=${effective_version}" >>"${GITHUB_OUTPUT}"
fi

