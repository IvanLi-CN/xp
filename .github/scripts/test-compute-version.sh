#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
compute_script="${script_dir}/compute-version.sh"

tmp_root="$(mktemp -d)"
trap 'rm -rf "${tmp_root}"' EXIT

create_repo() {
  local repo="$1"

  mkdir -p "${repo}"
  git -C "${repo}" init -q
  git -C "${repo}" config user.name "Test User"
  git -C "${repo}" config user.email "test@example.com"

  cat >"${repo}/Cargo.toml" <<'EOF'
[package]
name = "xp"
version = "0.2.0"
edition = "2024"
EOF

  echo "seed" >"${repo}/README.md"
  git -C "${repo}" add Cargo.toml README.md
  git -C "${repo}" commit -q -m "init"
}

commit_file() {
  local repo="$1"
  local path="$2"
  local content="$3"
  local message="$4"

  printf '%s\n' "${content}" >"${repo}/${path}"
  git -C "${repo}" add "${path}"
  git -C "${repo}" commit -q -m "${message}"
}

assert_version() {
  local label="$1"
  local repo="$2"
  local target_sha="$3"
  local expected="$4"
  local prerelease="${5:-false}"
  local output
  local actual

  output="$(
    cd "${repo}"
    RELEASE_HEAD_SHA="${target_sha}" \
      BUMP_LEVEL=patch \
      IS_PRERELEASE="${prerelease}" \
      "${compute_script}"
  )"

  actual="$(printf '%s\n' "${output}" | sed -n 's/^XP_EFFECTIVE_VERSION=//p' | tail -n 1)"
  if [[ "${actual}" != "${expected}" ]]; then
    echo "[FAIL] ${label}: expected ${expected}, got ${actual}" >&2
    printf '%s\n' "${output}" >&2
    exit 1
  fi

  echo "[PASS] ${label}: ${actual}"
}

repo_exact="${tmp_root}/exact"
create_repo "${repo_exact}"
git -C "${repo_exact}" tag -a v3.5.0 -m v3.5.0 HEAD
commit_file "${repo_exact}" feature.txt "target" "target release"
target_exact_sha="$(git -C "${repo_exact}" rev-parse HEAD)"
git -C "${repo_exact}" tag -a v3.5.1 -m v3.5.1 "${target_exact_sha}"
commit_file "${repo_exact}" followup.txt "later" "later release"
git -C "${repo_exact}" tag -a v3.5.2 -m v3.5.2 HEAD
assert_version \
  "reuse exact release tag on backfill target" \
  "${repo_exact}" \
  "${target_exact_sha}" \
  "3.5.1"

repo_ancestor="${tmp_root}/ancestor"
create_repo "${repo_ancestor}"
git -C "${repo_ancestor}" tag -a v3.5.0 -m v3.5.0 HEAD
commit_file "${repo_ancestor}" feature.txt "target" "target release"
target_ancestor_sha="$(git -C "${repo_ancestor}" rev-parse HEAD)"
commit_file "${repo_ancestor}" followup.txt "later" "later release"
git -C "${repo_ancestor}" tag -a v3.5.2 -m v3.5.2 HEAD
assert_version \
  "derive backfill version from target history instead of latest tag" \
  "${repo_ancestor}" \
  "${target_ancestor_sha}" \
  "3.5.1"

repo_stable_collision="${tmp_root}/stable-collision"
create_repo "${repo_stable_collision}"
git -C "${repo_stable_collision}" tag -a v1.0.0 -m v1.0.0 HEAD
commit_file "${repo_stable_collision}" feature.txt "target" "target release"
target_stable_collision_sha="$(git -C "${repo_stable_collision}" rev-parse HEAD)"
commit_file "${repo_stable_collision}" followup.txt "later" "later release"
git -C "${repo_stable_collision}" tag -a v1.0.1 -m v1.0.1 HEAD
commit_file "${repo_stable_collision}" extra.txt "later-2" "later release two"
git -C "${repo_stable_collision}" tag -a v1.0.2 -m v1.0.2 HEAD
assert_version \
  "keep stable backfills on the target patch line" \
  "${repo_stable_collision}" \
  "${target_stable_collision_sha}" \
  "1.0.1"

repo_cargo_fallback="${tmp_root}/cargo-fallback"
create_repo "${repo_cargo_fallback}"
target_cargo_fallback_sha="$(git -C "${repo_cargo_fallback}" rev-parse HEAD)"
commit_file "${repo_cargo_fallback}" feature.txt "later" "later release"
git -C "${repo_cargo_fallback}" tag -a v3.5.0 -m v3.5.0 HEAD
assert_version \
  "fall back to Cargo.toml before the first release tag" \
  "${repo_cargo_fallback}" \
  "${target_cargo_fallback_sha}" \
  "0.2.1"

repo_prerelease="${tmp_root}/prerelease"
create_repo "${repo_prerelease}"
git -C "${repo_prerelease}" tag -a v1.0.0 -m v1.0.0 HEAD
commit_file "${repo_prerelease}" feature.txt "target" "target prerelease"
target_prerelease_sha="$(git -C "${repo_prerelease}" rev-parse HEAD)"
commit_file "${repo_prerelease}" future.txt "unrelated" "future stable"
git -C "${repo_prerelease}" tag -a v1.1.0 -m v1.1.0 HEAD
assert_version \
  "derive prerelease backfills from the target history" \
  "${repo_prerelease}" \
  "${target_prerelease_sha}" \
  "1.0.1-rc.1" \
  "true"

repo_prerelease_stable_collision="${tmp_root}/prerelease-stable-collision"
create_repo "${repo_prerelease_stable_collision}"
git -C "${repo_prerelease_stable_collision}" tag -a v1.0.0 -m v1.0.0 HEAD
commit_file "${repo_prerelease_stable_collision}" feature.txt "target" "target prerelease"
target_prerelease_stable_collision_sha="$(git -C "${repo_prerelease_stable_collision}" rev-parse HEAD)"
commit_file "${repo_prerelease_stable_collision}" followup.txt "later" "later stable"
git -C "${repo_prerelease_stable_collision}" tag -a v1.0.1 -m v1.0.1 HEAD
assert_version \
  "keep prerelease backfills on the target patch line" \
  "${repo_prerelease_stable_collision}" \
  "${target_prerelease_stable_collision_sha}" \
  "1.0.1-rc.1" \
  "true"

repo_prerelease_collision="${tmp_root}/prerelease-collision"
create_repo "${repo_prerelease_collision}"
git -C "${repo_prerelease_collision}" tag -a v1.0.0 -m v1.0.0 HEAD
commit_file "${repo_prerelease_collision}" feature.txt "target" "target prerelease"
target_prerelease_collision_sha="$(git -C "${repo_prerelease_collision}" rev-parse HEAD)"
commit_file "${repo_prerelease_collision}" followup.txt "later" "later prerelease"
git -C "${repo_prerelease_collision}" tag -a v1.0.1-rc.1 -m v1.0.1-rc.1 HEAD
assert_version \
  "bump prerelease backfills to the next globally free rc" \
  "${repo_prerelease_collision}" \
  "${target_prerelease_collision_sha}" \
  "1.0.1-rc.2" \
  "true"
