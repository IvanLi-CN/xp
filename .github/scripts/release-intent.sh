#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mode="auto"
if [[ "${RELEASE_MODE:-auto}" == "manual" ]]; then
  mode="manual"
fi

exec python3 "${script_dir}/release_intent.py" \
  --mode "${mode}" \
  --repository "${GITHUB_REPOSITORY:-}" \
  --token "${GITHUB_TOKEN:-}" \
  --api-root "${GITHUB_API_URL:-https://api.github.com}" \
  --sha "${WORKFLOW_RUN_SHA:-${GITHUB_SHA:-}}" \
  --release-type "${RELEASE_TYPE:-}" \
  --channel "${RELEASE_CHANNEL:-}" \
  --expected-version "${EXPECTED_VERSION:-}" \
  --reason "${RELEASE_REASON:-}"
