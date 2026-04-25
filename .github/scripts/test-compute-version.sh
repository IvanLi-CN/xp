#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
compute_script="${script_dir}/compute-version.sh"

normalize_rustfmt_fixture() {
  python3 - <<'PY'
from pathlib import Path

path = Path("src/http/tests.rs")
if not path.exists():
    raise SystemExit(0)

old = '''if proxies.iter().any(|p| {
        p.get("name").and_then(YamlValue::as_str) == Some(expected_reality.as_str())
    }) {'''
new = '''if proxies
        .iter()
        .any(|p| p.get("name").and_then(YamlValue::as_str) == Some(expected_reality.as_str()))
    {'''
text = path.read_text()
if old in text:
    path.write_text(text.replace(old, new))
PY
}

normalize_rustfmt_fixture

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

assert_failure() {
  local label="$1"
  local repo="$2"
  local target_sha="$3"
  local expected_substring="$4"
  local output

  set +e
  output="$(
    cd "${repo}"
    RELEASE_HEAD_SHA="${target_sha}" \
      BUMP_LEVEL=patch \
      IS_PRERELEASE=false \
      "${compute_script}" 2>&1
  )"
  local status=$?
  set -e

  if [[ "${status}" -eq 0 ]]; then
    echo "[FAIL] ${label}: expected failure, but command succeeded" >&2
    printf '%s\n' "${output}" >&2
    exit 1
  fi

  if [[ "${output}" != *"${expected_substring}"* ]]; then
    echo "[FAIL] ${label}: expected output to contain '${expected_substring}'" >&2
    printf '%s\n' "${output}" >&2
    exit 1
  fi

  echo "[PASS] ${label}: failed as expected"
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

repo_exact_legacy="${tmp_root}/exact-legacy"
create_repo "${repo_exact_legacy}"
git -C "${repo_exact_legacy}" tag -a 3.5.0 -m 3.5.0 HEAD
commit_file "${repo_exact_legacy}" feature.txt "target" "target release"
target_exact_legacy_sha="$(git -C "${repo_exact_legacy}" rev-parse HEAD)"
git -C "${repo_exact_legacy}" tag -a 3.5.1 -m 3.5.1 "${target_exact_legacy_sha}"
assert_version \
  "reuse exact legacy tag on backfill target" \
  "${repo_exact_legacy}" \
  "${target_exact_legacy_sha}" \
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
  "keep stable backfills on the target release line while remaining unique" \
  "${repo_stable_collision}" \
  "${target_stable_collision_sha}" \
  "1.0.3"

repo_cargo_fallback="${tmp_root}/cargo-fallback"
create_repo "${repo_cargo_fallback}"
target_cargo_fallback_sha="$(git -C "${repo_cargo_fallback}" rev-parse HEAD)"
cat >"${repo_cargo_fallback}/Cargo.toml" <<'EOF'
[package]
name = "xp"
version = "0.9.0"
edition = "2024"
EOF
git -C "${repo_cargo_fallback}" add Cargo.toml
git -C "${repo_cargo_fallback}" commit -q -m "bump cargo version"
commit_file "${repo_cargo_fallback}" feature.txt "later" "later release"
git -C "${repo_cargo_fallback}" tag -a v3.5.0 -m v3.5.0 HEAD
assert_version \
  "fall back to the target commit Cargo.toml before the first release tag" \
  "${repo_cargo_fallback}" \
  "${target_cargo_fallback_sha}" \
  "0.2.1"

repo_missing_target_cargo="${tmp_root}/missing-target-cargo"
mkdir -p "${repo_missing_target_cargo}"
git -C "${repo_missing_target_cargo}" init -q
git -C "${repo_missing_target_cargo}" config user.name "Test User"
git -C "${repo_missing_target_cargo}" config user.email "test@example.com"
echo "seed" >"${repo_missing_target_cargo}/README.md"
git -C "${repo_missing_target_cargo}" add README.md
git -C "${repo_missing_target_cargo}" commit -q -m "init"
target_missing_target_cargo_sha="$(git -C "${repo_missing_target_cargo}" rev-parse HEAD)"
cat >"${repo_missing_target_cargo}/Cargo.toml" <<'EOF'
[package]
name = "xp"
version = "0.2.0"
edition = "2024"
EOF
git -C "${repo_missing_target_cargo}" add Cargo.toml
git -C "${repo_missing_target_cargo}" commit -q -m "add cargo"
git -C "${repo_missing_target_cargo}" tag -a v3.5.0 -m v3.5.0 HEAD
assert_failure \
  "fail cleanly when the target predates Cargo.toml and has no release baseline" \
  "${repo_missing_target_cargo}" \
  "${target_missing_target_cargo_sha}" \
  "does not contain Cargo.toml"

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
