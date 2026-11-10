#!/usr/bin/env bash
set -euo pipefail

# Compile and run every test target. The two tests that mutate shared upgrade
# fixtures are skipped from the parallel pass and rerun serially below.
cargo test --tests --no-run

test_status=0
upgrade_skip_args=()
while IFS= read -r test_name; do
  upgrade_skip_args+=(--skip "$test_name")
done < <(rg -o '^(async )?fn [A-Za-z0-9_]+' tests/xp_ops_upgrade.rs | awk '{print $NF}')
cargo test --tests --no-fail-fast -- --test-threads=16 \
  --skip internal_restore_existing_node_starts_durable_lifecycle_operation \
  "${upgrade_skip_args[@]}" &
test_pids=("$!")
cargo test --lib --no-fail-fast \
  internal_restore_existing_node_starts_durable_lifecycle_operation -- --test-threads=1 &
test_pids+=("$!")
cargo test --test xp_ops_upgrade --no-fail-fast -- --test-threads=1 &
test_pids+=("$!")
cargo test --bins --no-fail-fast -- --test-threads=12 &
test_pids+=("$!")
cargo test --doc --no-fail-fast -- --test-threads=12 &
test_pids+=("$!")
for pid in "${test_pids[@]}"; do
  wait "$pid" || test_status=1
done

if (( test_status != 0 )); then
  exit 1
fi
