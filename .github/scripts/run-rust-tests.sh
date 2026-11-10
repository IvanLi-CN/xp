#!/usr/bin/env bash
set -euo pipefail

# Compile every test target once. Run the compiled integration binaries directly
# so Cargo does not serialize target scheduling; xp_ops_upgrade owns shared
# upgrade fixtures and therefore runs as one serial binary.
cargo test --tests --no-run

test_status=0
cargo test --lib --no-fail-fast -- --test-threads=12 \
  --skip internal_restore_existing_node_starts_durable_lifecycle_operation &
test_pids=("$!")
cargo test --lib --no-fail-fast \
  internal_restore_existing_node_starts_durable_lifecycle_operation -- --test-threads=1 &
test_pids+=("$!")
cargo test --bins --no-fail-fast -- --test-threads=12 &
test_pids+=("$!")
cargo test --doc --no-fail-fast -- --test-threads=12 &
test_pids+=("$!")

while IFS= read -r target; do
  executable=$(find target/debug/deps -maxdepth 1 -type f -perm -111 -name "${target}-*" | head -n 1)
  test -n "$executable"
  threads=12
  if [[ "$target" == "xp_ops_upgrade" ]]; then
    threads=1
  fi
  "$executable" --test-threads="$threads" &
  test_pids+=("$!")
done < <(cargo metadata --no-deps --format-version 1 | python3 -c '
import json
import sys
metadata = json.load(sys.stdin)
for package in metadata["packages"]:
    for target in package["targets"]:
        if "test" in target["kind"] and target["name"] != package["name"]:
            print(target["name"])
')

for pid in "${test_pids[@]}"; do
  wait "$pid" || test_status=1
done

if (( test_status != 0 )); then
  exit 1
fi
