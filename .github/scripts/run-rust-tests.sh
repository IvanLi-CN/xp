#!/usr/bin/env bash
set -euo pipefail

# Compile every test target once, then run the unit-test binary and all integration
# targets concurrently inside the same required job. No test target is omitted.
cargo test --tests --no-run

cargo test --lib --no-fail-fast -- --test-threads=12 \
  --skip internal_restore_existing_node_starts_durable_lifecycle_operation
cargo test --lib --no-fail-fast \
  internal_restore_existing_node_starts_durable_lifecycle_operation -- --test-threads=1
cargo test --bins --no-fail-fast -- --test-threads=12
cargo test --doc --no-fail-fast -- --test-threads=12

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
integration_pids=()
for target in "${integration_targets[@]}"; do
  cargo test --test "$target" --no-fail-fast -- --test-threads=12 &
  integration_pids+=("$!")
done
for pid in "${integration_pids[@]}"; do
  wait "$pid" || integration_status=1
done

if (( integration_status != 0 )); then
  exit 1
fi
