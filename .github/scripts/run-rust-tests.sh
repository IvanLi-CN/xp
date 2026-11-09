#!/usr/bin/env bash
set -euo pipefail

# Compile every test target once, then run the unit-test binary and all integration
# targets concurrently inside the same required job. No test target is omitted.
cargo test --tests --no-run

cargo test --lib --no-fail-fast &
unit_pid=$!

integration_targets=()
while IFS= read -r target; do
  integration_targets+=("$target")
done < <(
  cargo metadata --no-deps --format-version 1 | python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
for package in metadata["packages"]:
    for target in package["targets"]:
        if "test" in target["kind"] and target["name"] != package["name"]:
            print(target["name"])
'
)

integration_status=0
for target in "${integration_targets[@]}"; do
  cargo test --test "$target" --no-fail-fast || integration_status=1
done &
integration_pid=$!

unit_status=0
wait "$unit_pid" || unit_status=$?
wait "$integration_pid" || integration_status=$?

if (( unit_status != 0 || integration_status != 0 )); then
  exit 1
fi
