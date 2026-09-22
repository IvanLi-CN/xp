#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: %s --cache-root DIR --cargo-home DIR --slot candidate|baseline --source DIR -- cargo ...\n' \
    "$(basename "$0")"
}

die() {
  printf 'cargo-cache: %s\n' "$*" >&2
  exit 64
}

cache_root=''
cargo_home=''
slot=''
source_dir=''

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cache-root)
      [[ $# -ge 2 ]] || die '--cache-root requires a value'
      cache_root="$2"
      shift 2
      ;;
    --cargo-home)
      [[ $# -ge 2 ]] || die '--cargo-home requires a value'
      cargo_home="$2"
      shift 2
      ;;
    --slot)
      [[ $# -ge 2 ]] || die '--slot requires a value'
      slot="$2"
      shift 2
      ;;
    --source)
      [[ $# -ge 2 ]] || die '--source requires a value'
      source_dir="$2"
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

[[ -n "$cache_root" ]] || die '--cache-root is required'
[[ -n "$cargo_home" ]] || die '--cargo-home is required'
[[ -n "$slot" ]] || die '--slot is required'
[[ -n "$source_dir" ]] || die '--source is required'
[[ $# -gt 0 ]] || die 'a Cargo command is required after --'

case "$cache_root:$cargo_home:$source_dir" in
  /*:/*:/*) ;;
  *) die 'cache-root, cargo-home and source must be absolute paths' ;;
esac
if [[ -L "$cache_root" || -L "$cargo_home" ]]; then
  die 'cache-root and cargo-home must not be symlinks'
fi
case "$slot" in
  candidate|baseline) ;;
  *) die "unsupported slot: $slot" ;;
esac
[[ "$1" == cargo ]] || die 'the command after -- must be the cargo command name'
[[ -d "$source_dir" && ! -L "$source_dir" ]] || die "source is not a directory: $source_dir"

command -v cargo >/dev/null 2>&1 || die 'cargo is not available'
command -v rustc >/dev/null 2>&1 || die 'rustc is not available'
if command -v sha256sum >/dev/null 2>&1; then
  checksum_stream() { sha256sum | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  checksum_stream() { shasum -a 256 | awk '{print $1}'; }
else
  die 'sha256sum or shasum is not available'
fi

toolchain_key="$(rustc -Vv | checksum_stream | cut -c1-16)"
target_dir="$cache_root/target/$toolchain_key/$slot"
lock_path="$target_dir/.cargo-cache.lock"
lock_dir="$lock_path.d"
reject_cache_symlinks() {
  local path="$1"
  while [[ "$path" != "$cache_root" && "$path" != / ]]; do
    [[ ! -L "$path" ]] || die "cache path contains a symlink: $path"
    path="$(dirname "$path")"
  done
}
reject_cache_symlinks "$target_dir"
mkdir -p "$target_dir" "$cargo_home"
[[ ! -L "$lock_path" && ! -L "$lock_dir" ]] || die 'cache lock path must not be a symlink'

cleanup_lock() {
  if [[ "${LOCK_ACQUIRED:-0}" == 1 && "${LOCK_MODE:-}" == mkdir ]]; then
    rm -f "$lock_dir/pid"
    rmdir "$lock_dir" 2>/dev/null || true
  fi
}
trap cleanup_lock EXIT

if command -v flock >/dev/null 2>&1; then
  exec 9>"$lock_path"
  flock -x 9
  LOCK_MODE=flock
  LOCK_ACQUIRED=1
else
  lock_deadline=$((SECONDS + 120))
  while ! mkdir "$lock_dir" 2>/dev/null; do
    if [[ -r "$lock_dir/pid" ]]; then
      lock_pid="$(cat "$lock_dir/pid" 2>/dev/null || true)"
      if [[ -n "$lock_pid" ]] && ! kill -0 "$lock_pid" 2>/dev/null; then
        rm -f "$lock_dir/pid"
        rmdir "$lock_dir" 2>/dev/null || true
        continue
      fi
    fi
    if (( SECONDS >= lock_deadline )); then
      die "timed out waiting for $lock_dir"
    fi
    sleep 1
  done
  LOCK_MODE='mkdir'
  LOCK_ACQUIRED=1
  printf '%s\n' "$$" > "$lock_dir/pid"
fi
printf 'cargo_cache_slot=%s\n' "$slot" >&2
printf 'cargo_cache_toolchain=%s\n' "$toolchain_key" >&2
printf 'cargo_cache_target=%s\n' "$target_dir" >&2

cd "$source_dir"
export CARGO_HOME="$cargo_home"
export CARGO_TARGET_DIR="$target_dir"
"$@"
