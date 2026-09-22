#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: %s --cache-root DIR [--slot candidate|baseline]\n' "$(basename "$0")"
}

die() {
  printf 'cargo-cache-status: %s\n' "$*" >&2
  exit 64
}

cache_root=''
slot=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --cache-root)
      [[ $# -ge 2 ]] || die '--cache-root requires a value'
      cache_root="$2"
      shift 2
      ;;
    --slot)
      [[ $# -ge 2 ]] || die '--slot requires a value'
      slot="$2"
      shift 2
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
[[ "$cache_root" == /* ]] || die 'cache-root must be absolute'
if [[ -n "$slot" && "$slot" != candidate && "$slot" != baseline ]]; then
  die "unsupported slot: $slot"
fi

if [[ ! -d "$cache_root" || -L "$cache_root" ]]; then
  printf 'cache_root=%s\ncache_root_state=absent\n' "$cache_root"
  exit 0
fi

command -v rustc >/dev/null 2>&1 || die 'rustc is not available'
if command -v sha256sum >/dev/null 2>&1; then
  checksum_stream() { sha256sum | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  checksum_stream() { shasum -a 256 | awk '{print $1}'; }
else
  die 'sha256sum or shasum is not available'
fi
toolchain_key="$(rustc -Vv | checksum_stream | cut -c1-16)"
printf 'cache_root=%s\n' "$cache_root"
printf 'toolchain=%s\n' "$toolchain_key"

status_slot() {
  local name="$1"
  local target_dir="$cache_root/target/$toolchain_key/$name"
  local source_dir="$cache_root/source/$name"
  local lock_path="$target_dir/.cargo-cache.lock"
  printf 'slot=%s\n' "$name"
  if [[ -d "$target_dir" ]]; then
    printf 'target_state=present\n'
    du -sh "$target_dir" 2>/dev/null | awk '{print "target_size=" $1}' || true
  else
    printf 'target_state=absent\n'
  fi
  if [[ -f "$source_dir/.xp-source-marker" ]]; then
    printf 'source_marker=%s\n' "$(tr '\n' ' ' < "$source_dir/.xp-source-marker")"
  else
    printf 'source_marker=absent\n'
  fi
  if [[ -f "$lock_path" ]] && command -v flock >/dev/null 2>&1; then
    exec 8<"$lock_path"
    if flock -n 8; then
      printf 'lock_state=free\n'
    else
      printf 'lock_state=busy\n'
    fi
    exec 8<&-
  elif [[ -d "$lock_path.d" ]]; then
    printf 'lock_state=busy\n'
  elif [[ -f "$lock_path" ]]; then
    printf 'lock_state=unknown\n'
  else
    printf 'lock_state=absent\n'
  fi
}

if [[ -n "$slot" ]]; then
  status_slot "$slot"
else
  status_slot candidate
  status_slot baseline
fi
