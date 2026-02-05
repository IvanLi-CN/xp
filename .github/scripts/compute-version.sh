#!/usr/bin/env bash
set -euo pipefail

# Compute the next effective semver version by:
# - base version: max semver tag (accepts v<semver> and <semver>), fallback Cargo.toml version
# - bump: major/minor/patch, controlled by $BUMP_LEVEL
# - prerelease: when $IS_PRERELEASE=true, emit <base>-rc.<N> and mark GitHub release as prerelease
# - uniqueness: if tag exists (including legacy w/ or w/o leading v), keep incrementing patch/rc until free
#
# Outputs:
# - XP_EFFECTIVE_VERSION=<semver> (no leading v)
# - GitHub Actions: steps.<id>.outputs.version=<semver>

root_dir="$(git rev-parse --show-toplevel)"

git fetch --tags --force >/dev/null 2>&1 || true

cargo_ver="$(
  awk '
    $0 ~ /^\[package\]/ { in_pkg=1; next }
    $0 ~ /^\[/ { in_pkg=0 }
    in_pkg && $1 == "version" {
      gsub(/"/, "", $3);
      print $3;
      exit
    }
  ' "${root_dir}/Cargo.toml"
)"

if [[ -z "${cargo_ver:-}" ]]; then
  echo "failed to parse [package].version from Cargo.toml" >&2
  exit 1
fi

if [[ -z "${BUMP_LEVEL:-}" ]]; then
  echo "missing BUMP_LEVEL (expected: major|minor|patch)" >&2
  exit 1
fi

if [[ "${BUMP_LEVEL}" != "major" && "${BUMP_LEVEL}" != "minor" && "${BUMP_LEVEL}" != "patch" ]]; then
  echo "invalid BUMP_LEVEL=${BUMP_LEVEL} (expected: major|minor|patch)" >&2
  exit 1
fi

is_prerelease="${IS_PRERELEASE:-false}"
if [[ "${is_prerelease}" != "true" && "${is_prerelease}" != "false" ]]; then
  echo "invalid IS_PRERELEASE=${is_prerelease} (expected: true|false)" >&2
  exit 1
fi

max_tag="$(
  git tag -l \
    | grep -E '^v?[0-9]+\.[0-9]+\.[0-9]+$' \
    | sed -E 's/^v//' \
    | sort -Vu \
    | tail -n 1 \
    || true
)"

base_ver="${max_tag:-$cargo_ver}"

base_major="$(echo "$base_ver" | cut -d. -f1)"
base_minor="$(echo "$base_ver" | cut -d. -f2)"
base_patch="$(echo "$base_ver" | cut -d. -f3)"

case "${BUMP_LEVEL}" in
  major)
    next_major="$((base_major + 1))"
    next_minor="0"
    next_patch="0"
    ;;
  minor)
    next_major="${base_major}"
    next_minor="$((base_minor + 1))"
    next_patch="0"
    ;;
  patch)
    next_major="${base_major}"
    next_minor="${base_minor}"
    next_patch="$((base_patch + 1))"
    ;;
esac

candidate="${next_patch}"
while \
  git rev-parse -q --verify "refs/tags/v${next_major}.${next_minor}.${candidate}" >/dev/null \
  || git rev-parse -q --verify "refs/tags/${next_major}.${next_minor}.${candidate}" >/dev/null; do
  candidate="$((candidate + 1))"
done

effective_version="${next_major}.${next_minor}.${candidate}"
if [[ "${is_prerelease}" == "true" ]]; then
  rc=1
  while \
    git rev-parse -q --verify "refs/tags/v${effective_version}-rc.${rc}" >/dev/null \
    || git rev-parse -q --verify "refs/tags/${effective_version}-rc.${rc}" >/dev/null; do
    rc="$((rc + 1))"
  done
  effective_version="${effective_version}-rc.${rc}"
fi

echo "XP_EFFECTIVE_VERSION=${effective_version}"
echo "Computed XP_EFFECTIVE_VERSION=${effective_version}"
echo "  base_version=${base_ver} (max_tag=${max_tag:-<none>}, cargo=${cargo_ver})"
echo "  bump_level=${BUMP_LEVEL}"
echo "  is_prerelease=${is_prerelease}"

if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "XP_EFFECTIVE_VERSION=${effective_version}" >>"${GITHUB_ENV}"
fi

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  echo "version=${effective_version}" >>"${GITHUB_OUTPUT}"
fi
