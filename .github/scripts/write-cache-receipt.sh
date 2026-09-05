#!/usr/bin/env bash
set -euo pipefail

sample="${1:?sample is required}"
phase="${2:?phase is required}"
component="${3:?component is required}"
cache_hit="${4:?cache hit is required}"
output="${5:?output is required}"

if [[ "${cache_hit}" != true && "${cache_hit}" != false ]]; then
  echo "cache hit must be true or false" >&2
  exit 1
fi

mkdir -p "$(dirname "${output}")"
jq -n \
  --arg sample "${sample}" \
  --arg phase "${phase}" \
  --arg component "${component}" \
  --argjson cache_hit "${cache_hit}" \
  '{sample:$sample,phase:$phase,component:$component,cache_hit:$cache_hit}' \
  > "${output}"
