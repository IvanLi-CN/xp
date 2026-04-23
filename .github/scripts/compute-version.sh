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

target_sha="${RELEASE_HEAD_SHA:-${WORKFLOW_RUN_SHA:-${GITHUB_SHA:-}}}"

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

if [[ -n "${target_sha}" ]] && ! git rev-parse -q --verify "${target_sha}^{commit}" >/dev/null; then
  echo "invalid release target sha=${target_sha}" >&2
  exit 1
fi

list_normalized_semver_tags_on_commit() {
  local sha="$1"
  local pattern="$2"

  git tag --points-at "${sha}" \
    | grep -E "${pattern}" \
    | sed -E 's/^v//' \
    | sort -Vu \
    || true
}

pick_unique_version() {
  local values="$1"
  local unique_count

  unique_count="$(
    printf '%s\n' "${values}" \
      | sed '/^$/d' \
      | wc -l \
      | tr -d ' '
  )"

  if [[ "${unique_count}" -eq 0 ]]; then
    return 0
  fi

  if [[ "${unique_count}" -ne 1 ]]; then
    echo "ambiguous release version candidates:" >&2
    printf '%s\n' "${values}" >&2
    exit 1
  fi

  printf '%s\n' "${values}" | sed -n '1p'
}

resolve_exact_release_version() {
  local sha="$1"
  local pattern

  if [[ -z "${sha}" ]]; then
    return 0
  fi

  if [[ "${is_prerelease}" == "true" ]]; then
    pattern='^v?[0-9]+\.[0-9]+\.[0-9]+-rc\.[0-9]+$'
  else
    pattern='^v?[0-9]+\.[0-9]+\.[0-9]+$'
  fi

  pick_unique_version "$(list_normalized_semver_tags_on_commit "${sha}" "${pattern}")"
}

resolve_previous_release_tag() {
  local sha="$1"
  local commit_tags

  if [[ -z "${sha}" ]]; then
    return 0
  fi

  while IFS= read -r commit; do
    commit_tags="$(
      list_normalized_semver_tags_on_commit "${commit}" '^v?[0-9]+\.[0-9]+\.[0-9]+$'
    )"
    if [[ -n "${commit_tags}" ]]; then
      pick_unique_version "${commit_tags}"
      return 0
    fi
  done < <(git rev-list --first-parent "${sha}^" 2>/dev/null || true)
}

resolve_previous_prerelease_rc() {
  local sha="$1"
  local version="$2"
  local version_pattern="${version//./\\.}"
  local commit
  local prerelease_tags
  local max_rc="0"
  local rc

  if [[ -z "${sha}" || -z "${version}" ]]; then
    echo "0"
    return 0
  fi

  while IFS= read -r commit; do
    prerelease_tags="$(
      list_normalized_semver_tags_on_commit \
        "${commit}" \
        "^v?${version_pattern}-rc\\.[0-9]+$"
    )"
    if [[ -z "${prerelease_tags}" ]]; then
      continue
    fi

    while IFS= read -r tag; do
      rc="${tag##*-rc.}"
      if [[ -n "${rc}" && "${rc}" -gt "${max_rc}" ]]; then
        max_rc="${rc}"
      fi
    done <<<"${prerelease_tags}"
  done < <(git rev-list --first-parent "${sha}^" 2>/dev/null || true)

  echo "${max_rc}"
}

max_tag="$(
  git tag -l \
    | grep -E '^v?[0-9]+\.[0-9]+\.[0-9]+$' \
    | sed -E 's/^v//' \
    | sort -Vu \
    | tail -n 1 \
    || true
)"

exact_version="$(resolve_exact_release_version "${target_sha}")"

if [[ -n "${exact_version}" ]]; then
  effective_version="${exact_version}"
  version_source="exact_tag"
  base_ver="${exact_version}"
else
  if [[ -n "${target_sha}" ]]; then
    base_ver="$(resolve_previous_release_tag "${target_sha}")"
    if [[ -n "${base_ver}" ]]; then
      version_source="previous_release_on_target_history"
    else
      base_ver="${cargo_ver}"
      version_source="cargo_version_fallback"
    fi
  else
    base_ver="${max_tag:-$cargo_ver}"
    version_source="global_max_tag"
  fi

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
  if [[ "${is_prerelease}" != "true" || -z "${target_sha}" ]]; then
    while \
      git rev-parse -q --verify "refs/tags/v${next_major}.${next_minor}.${candidate}" >/dev/null \
      || git rev-parse -q --verify "refs/tags/${next_major}.${next_minor}.${candidate}" >/dev/null; do
      candidate="$((candidate + 1))"
    done
  fi

  effective_version="${next_major}.${next_minor}.${candidate}"
  if [[ "${is_prerelease}" == "true" ]]; then
    if [[ -n "${target_sha}" ]]; then
      rc="$(( $(resolve_previous_prerelease_rc "${target_sha}" "${effective_version}") + 1 ))"
    else
      rc=1
    fi

    while \
      git rev-parse -q --verify "refs/tags/v${effective_version}-rc.${rc}" >/dev/null \
      || git rev-parse -q --verify "refs/tags/${effective_version}-rc.${rc}" >/dev/null; do
      rc="$((rc + 1))"
    done

    effective_version="${effective_version}-rc.${rc}"
  fi
fi

echo "XP_EFFECTIVE_VERSION=${effective_version}"
echo "Computed XP_EFFECTIVE_VERSION=${effective_version}"
echo "  base_version=${base_ver} (max_tag=${max_tag:-<none>}, cargo=${cargo_ver})"
echo "  bump_level=${BUMP_LEVEL}"
echo "  is_prerelease=${is_prerelease}"
echo "  target_sha=${target_sha:-<none>}"
echo "  version_source=${version_source}"

if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "XP_EFFECTIVE_VERSION=${effective_version}" >>"${GITHUB_ENV}"
fi

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  echo "version=${effective_version}" >>"${GITHUB_OUTPUT}"
fi
