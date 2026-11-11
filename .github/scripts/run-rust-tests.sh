#!/usr/bin/env bash
set -euo pipefail

# Compile every test target once. Run the compiled integration binaries directly
# so Cargo does not serialize target scheduling; xp_ops_upgrade owns shared
# upgrade fixtures and therefore runs as one serial binary.
test_manifest=$(mktemp)
trap 'rm -f "$test_manifest" "$test_manifest.executables"' EXIT
cargo test --tests --no-run --message-format=json > "$test_manifest"

test_status=0
cargo test --lib --no-fail-fast -- --test-threads=12 \
  --skip internal_restore_existing_node_starts_durable_lifecycle_operation \
  --skip public_body_cancellation_and_lease_expiry_record_failure_telemetry &
test_pids=("$!")
cargo test --lib --no-fail-fast \
  internal_restore_existing_node_starts_durable_lifecycle_operation -- --test-threads=1 &
test_pids+=("$!")
cargo test --lib --no-fail-fast \
  public_body_cancellation_and_lease_expiry_record_failure_telemetry -- --test-threads=1 &
test_pids+=("$!")
cargo test --bins --no-fail-fast -- --test-threads=12 &
test_pids+=("$!")
cargo test --doc --no-fail-fast -- --test-threads=12 &
test_pids+=("$!")

python3 - "$test_manifest" > "$test_manifest.executables" <<'PYTHON'
import json
import sys

seen = set()
for line in open(sys.argv[1]):
    artifact = json.loads(line)
    target = artifact.get("target", {})
    executable = artifact.get("executable")
    if (artifact.get("reason") == "compiler-artifact"
            and "test" in target.get("kind", []) and executable
            and artifact.get("profile", {}).get("test") and executable not in seen):
        seen.add(executable)
        print(target["name"] + "\t" + executable)
if not seen:
    raise SystemExit("Cargo returned no integration test executables")
PYTHON
while IFS=$'\t' read -r target executable; do
  test -x "$executable"
  threads=12
  if [[ "$target" == "xp_ops_upgrade" ]]; then
    threads=1
  fi
  "$executable" --test-threads="$threads" &
  test_pids+=("$!")
done < "$test_manifest.executables"

for pid in "${test_pids[@]}"; do
  wait "$pid" || test_status=1
done

if (( test_status != 0 )); then
  exit 1
fi
