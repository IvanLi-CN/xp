#!/usr/bin/env bash
set -euo pipefail

epoch="$(git log -1 --format=%ct HEAD)"
timestamp="@${epoch}"

while IFS= read -r -d '' path; do
  touch -d "$timestamp" -- "$path"
done < <(git ls-files -z)

if [[ -d web/dist ]]; then
  while IFS= read -r -d '' path; do
    touch -d "$timestamp" -- "$path"
  done < <(find web/dist -type f -print0)
fi

if [[ -f web/.xp-dist-fingerprint ]]; then
  touch -d "$timestamp" -- web/.xp-dist-fingerprint
fi
