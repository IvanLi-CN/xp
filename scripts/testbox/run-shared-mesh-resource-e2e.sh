#!/usr/bin/env bash
set -euo pipefail

# Run the locked 50-peer Mesh resource gate on codex-testbox. The shared runner
# owns the isolated Compose project and cleans only resources created by this run.
export XP_RUN_MESH_RESOURCE=1
export XP_E2E_ONLY_MESH_RESOURCE=1
exec "$(dirname "$0")/run-shared-quota-xray-e2e.sh" "$@"
