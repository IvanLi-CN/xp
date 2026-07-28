#!/usr/bin/env bash
set -euo pipefail

duration=60
interval=1
limit=65536
while [[ $# -gt 0 ]]; do
  case "$1" in
    --duration-secs) duration="$2"; shift 2 ;;
    --interval-secs) interval="$2"; shift 2 ;;
    --limit-kib) limit="$2"; shift 2 ;;
    *) echo "usage: $0 [--duration-secs N] [--interval-secs N] [--limit-kib N]" >&2; exit 2 ;;
  esac
done

peak_xp=0; peak_xray=0; peak_cloudflared=0; peak_canary=0; peak_total=0
read_pss() {
  local pid="$1" source value
  if [[ -r "/proc/$pid/smaps_rollup" ]]; then
    source="/proc/$pid/smaps_rollup"
  elif [[ -r "/proc/$pid/smaps" ]]; then
    source="/proc/$pid/smaps"
  else
    printf '0\n'
    return
  fi
  value=$(awk '/^Pss:/ {sum += $2} END {print sum + 0}' "$source")
  printf '%s\n' "${value:-0}"
}
role_pss() {
  local role="$1" total=0 pid comm
  for pid in /proc/[0-9]*; do
    pid=${pid##*/}
    [[ -r "/proc/$pid/comm" ]] || continue
    comm=$(<"/proc/$pid/comm")
    case "$role:$comm" in
      xp:xp|xray:xray|cloudflared:cloudflared|canary:xp-vless-canary)
        value=$(read_pss "$pid"); total=$((total + value)) ;;
    esac
  done
  printf '%s\n' "$total"
}

start=$(date +%s)
while (( $(date +%s) - start < duration )); do
  xp=$(role_pss xp); xray=$(role_pss xray); cloudflared=$(role_pss cloudflared); canary=$(role_pss canary)
  total=$((xp + xray + cloudflared + canary))
  printf 'sample epoch=%s xp_kib=%s xray_kib=%s cloudflared_kib=%s canary_kib=%s total_kib=%s\n' "$(date +%s)" "$xp" "$xray" "$cloudflared" "$canary" "$total"
  for pair in "xp:$xp" "xray:$xray" "cloudflared:$cloudflared" "canary:$canary" "total:$total"; do
    role=${pair%%:*}; value=${pair##*:}
    case "$role" in
      xp) (( value > peak_xp )) && peak_xp=$value || true ;;
      xray) (( value > peak_xray )) && peak_xray=$value || true ;;
      cloudflared) (( value > peak_cloudflared )) && peak_cloudflared=$value || true ;;
      canary) (( value > peak_canary )) && peak_canary=$value || true ;;
      total) (( value > peak_total )) && peak_total=$value || true ;;
    esac
  done
  if (( total > limit )); then echo "budget_exceeded total_kib=$total limit_kib=$limit" >&2; exit 1; fi
  sleep "$interval"
done
printf 'peak xp_kib=%s xray_kib=%s cloudflared_kib=%s canary_kib=%s total_kib=%s limit_kib=%s\n' "$peak_xp" "$peak_xray" "$peak_cloudflared" "$peak_canary" "$peak_total" "$limit"
