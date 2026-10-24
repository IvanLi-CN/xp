#!/usr/bin/env bash
set -euo pipefail

# Run the locked 50-peer Mesh resource gate on the configured shared test host. The shared runner
# owns the isolated Compose project and cleans only resources created by this run.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=runner-env.sh
source "$SCRIPT_DIR/runner-env.sh"
export XP_RUN_MESH_RESOURCE=1
export XP_E2E_ONLY_MESH_RESOURCE=1
exec "$SCRIPT_DIR/run-shared-quota-xray-e2e.sh" "$@"
