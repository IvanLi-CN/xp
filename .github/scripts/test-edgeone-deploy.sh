#!/usr/bin/env bash
set -euo pipefail

root_dir="$(git rev-parse --show-toplevel)"
deploy_script="${root_dir}/.github/scripts/deploy-edgeone.sh"
fake_npx="${root_dir}/.github/scripts/test-fixtures/fake-edgeone-npx"
test_root="$(mktemp -d)"
trap 'rm -rf -- "${test_root}"' EXIT

mkdir -p "${test_root}/site"
mkdir -p "${test_root}/bin"
ln -s "${fake_npx}" "${test_root}/bin/npx"
count_file="${test_root}/retry-count"
args_file="${test_root}/args"

run_failure_case() {
  local name="$1"
  local stage="$2"
  local expected_class="$3"
  local fail_until="$4"
  local max_attempts="$5"
  local expected_attempts
  local log_file="${test_root}/${name}.log"
  local status

  expected_attempts=$((fail_until < max_attempts ? fail_until : max_attempts))
  : > "${count_file}"
  if PATH="${test_root}/bin:${PATH}" \
    EDGEONE_API_TOKEN='fixture-token' \
    EDGEONE_CLI_VERSION='9.9.9' \
    EDGEONE_NPX_BIN='/tmp/untrusted-npx' \
    EDGEONE_TEST_COUNT_FILE="${count_file}" \
    EDGEONE_TEST_ARGS_FILE="${args_file}" \
    EDGEONE_TEST_FAILURE_STAGE="${stage}" \
    EDGEONE_TEST_FAIL_UNTIL="${fail_until}" \
    EDGEONE_MAX_ATTEMPTS="${max_attempts}" \
    EDGEONE_RETRY_DELAYS_SECONDS='0 0' \
    bash "${deploy_script}" "${test_root}/site" makers-project "${name}" \
    > "${log_file}" 2>&1; then
    echo "expected ${name} to fail" >&2
    exit 1
  else
    status=$?
  fi
  test "${status}" = 7
  grep -Fq "EDGEONE_FAILURE_CLASS=${expected_class}" "${log_file}"
  test "$(<"${count_file}")" = "${expected_attempts}"
}

: > "${count_file}"
if PATH="${test_root}/bin:${PATH}" \
  EDGEONE_API_TOKEN='fixture-token' \
  EDGEONE_CLI_VERSION='9.9.9' \
  EDGEONE_NPX_BIN='/tmp/untrusted-npx' \
  EDGEONE_TEST_COUNT_FILE="${count_file}" \
  EDGEONE_TEST_ARGS_FILE="${args_file}" \
  EDGEONE_TEST_FAILURE_STAGE=preflight-endpoint \
  EDGEONE_TEST_FAIL_UNTIL=2 \
  EDGEONE_MAX_ATTEMPTS=3 \
  EDGEONE_RETRY_DELAYS_SECONDS='0 0' \
  bash "${deploy_script}" "${test_root}/site" makers-project web-production \
  > "${test_root}/retry-success.log" 2>&1; then
  test "$(<"${count_file}")" = 3
else
  echo "expected bounded preflight retry to succeed" >&2
  exit 1
fi
grep -Fq "edgeone@1.6.33 makers deploy ${test_root}/site" "${args_file}"

run_failure_case endpoint-unavailable preflight-endpoint endpoint_unavailable 9 2
run_failure_case auth-rejected auth-rejected credential_rejected 1 3
run_failure_case project-permission project-permission project_or_permission 1 3
run_failure_case deployment-ambiguous post-deploy deployment_ambiguous 1 3
run_failure_case generic-token-error ambiguous auth_or_endpoint_ambiguous 1 3
run_failure_case ambiguous-http-5xx ambiguous-http-5xx auth_or_endpoint_ambiguous 1 3

if grep -Eqi 'token (is )?invalid([.![:space:]]|$)' \
  "${test_root}/generic-token-error.log"; then
  echo "ambiguous output must not claim token invalidity" >&2
  exit 1
fi

echo "EdgeOne deployment classification tests passed"
