#!/usr/bin/env bash
set -euo pipefail

# Compile and run every test target. The two tests that mutate shared upgrade
# fixtures are skipped from the parallel pass and rerun serially below.
cargo test --tests --no-run

test_status=0
cargo test --tests --no-fail-fast -- --test-threads=12 \
  --skip internal_restore_existing_node_starts_durable_lifecycle_operation \
  --skip upgrade_preserves_custom_xray_api_listener_from_env &
test_pids=("$!")
cargo test --lib --no-fail-fast \
  internal_restore_existing_node_starts_durable_lifecycle_operation -- --test-threads=1 &
test_pids+=("$!")
cargo test --test xp_ops_upgrade --no-fail-fast \
  upgrade_preserves_custom_xray_api_listener_from_env -- --test-threads=1 &
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
