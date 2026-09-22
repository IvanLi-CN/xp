#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/xp-cargo-cache-test.XXXXXX")"
fixture_root="$(cd "$fixture_root" && pwd)"
trap 'rm -rf "$fixture_root"' EXIT

mkdir -p "$fixture_root/source" "$fixture_root/bin"
original_path="$PATH"
cat > "$fixture_root/bin/cargo" <<'FAKE_CARGO'
#!/usr/bin/env bash
set -euo pipefail
printf 'cwd=%s\n' "$PWD"
printf 'home=%s\n' "$CARGO_HOME"
printf 'target=%s\n' "$CARGO_TARGET_DIR"
FAKE_CARGO
chmod +x "$fixture_root/bin/cargo"

export PATH="$fixture_root/bin:$PATH"
candidate_output="$("$SCRIPT_DIR/with-cargo-target.sh" \
  --cache-root "$fixture_root/cache" \
  --cargo-home "$fixture_root/shared-cargo-home" \
  --slot candidate \
  --source "$fixture_root/source" \
  -- cargo build 2>/dev/null)"

grep -Fx "cwd=$fixture_root/source" <<< "$candidate_output"
grep -Fx "home=$fixture_root/shared-cargo-home" <<< "$candidate_output"
candidate_target="$(sed -n 's/^target=//p' <<< "$candidate_output")"
[[ "$candidate_target" == "$fixture_root/cache/target/"*"/candidate" ]]

baseline_output="$("$SCRIPT_DIR/with-cargo-target.sh" \
  --cache-root "$fixture_root/cache" \
  --cargo-home "$fixture_root/shared-cargo-home" \
  --slot baseline \
  --source "$fixture_root/source" \
  -- cargo test 2>/dev/null)"

baseline_target="$(sed -n 's/^target=//p' <<< "$baseline_output")"
[[ "$baseline_target" == "$fixture_root/cache/target/"*"/baseline" ]]
[[ "$candidate_target" != "$baseline_target" ]]

if "$SCRIPT_DIR/with-cargo-target.sh" \
  --cache-root "$fixture_root/cache" \
  --cargo-home "$fixture_root/shared-cargo-home" \
  --slot invalid \
  --source "$fixture_root/source" \
  -- cargo build >/dev/null 2>&1; then
  printf 'invalid slot was accepted\n' >&2
  exit 1
fi

"$SCRIPT_DIR/status.sh" --cache-root "$fixture_root/cache" --slot candidate |
  grep -E '^slot=candidate$|^target_state=present$|^lock_state=(free|absent)$'

mkdir -p "$fixture_root/project/src"
cat > "$fixture_root/project/Cargo.toml" <<'PROJECT_MANIFEST'
[package]
name = "cargo-cache-fixture"
version = "0.1.0"
edition = "2021"
build = "build.rs"
PROJECT_MANIFEST
cat > "$fixture_root/project/build.rs" <<'PROJECT_BUILD'
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=XP_CACHE_TEST_VERSION");
    println!("cargo:rerun-if-changed=.xp-source-marker");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let marker = fs::read_to_string(".xp-source-marker").unwrap();
    fs::write(
        out_dir.join("version.txt"),
        format!("{}{}", env::var("XP_CACHE_TEST_VERSION").unwrap(), marker),
    )
    .unwrap();
}
PROJECT_BUILD
printf 'marker-one\n' > "$fixture_root/project/.xp-source-marker"
cat > "$fixture_root/project/src/main.rs" <<'PROJECT_MAIN'
fn main() {
    print!("{}", include_str!(concat!(env!("OUT_DIR"), "/version.txt")));
}
PROJECT_MAIN
export PATH="$original_path"
first_output="$(XP_CACHE_TEST_VERSION=one "$SCRIPT_DIR/with-cargo-target.sh" \
  --cache-root "$fixture_root/real-cache" \
  --cargo-home "$fixture_root/real-cargo-home" \
  --slot candidate \
  --source "$fixture_root/project" \
  -- cargo run --offline --quiet)"
second_output="$(XP_CACHE_TEST_VERSION=two "$SCRIPT_DIR/with-cargo-target.sh" \
  --cache-root "$fixture_root/real-cache" \
  --cargo-home "$fixture_root/real-cargo-home" \
  --slot candidate \
  --source "$fixture_root/project" \
  -- cargo run --offline --quiet)"
[[ "$first_output" == onemarker-one ]]
printf 'marker-two\n' > "$fixture_root/project/.xp-source-marker"
third_output="$(XP_CACHE_TEST_VERSION=two "$SCRIPT_DIR/with-cargo-target.sh" \
  --cache-root "$fixture_root/real-cache" \
  --cargo-home "$fixture_root/real-cargo-home" \
  --slot candidate \
  --source "$fixture_root/project" \
  -- cargo run --offline --quiet)"
[[ "$second_output" == twomarker-one ]]
[[ "$third_output" == twomarker-two ]]

printf 'cargo-cache tests: passed\n'
