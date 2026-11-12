#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 <deploy-directory> <project-name> <target-label>" >&2
}

if [[ $# -ne 3 ]]; then
  usage
  exit 2
fi

deploy_directory="$1"
project_name="$2"
target_label="$3"

: "${EDGEONE_API_TOKEN:?repository secret EDGEONE_API_TOKEN is required}"

if [[ ! -d "${deploy_directory}" ]]; then
  echo "EdgeOne deploy directory does not exist: ${deploy_directory}" >&2
  exit 1
fi

if [[ -z "${project_name}" ]]; then
  echo "EdgeOne project name is required for ${target_label}" >&2
  exit 1
fi

cli_version="1.6.33"
npx_bin="npx"
max_attempts="${EDGEONE_MAX_ATTEMPTS:-3}"
retry_delays_text="${EDGEONE_RETRY_DELAYS_SECONDS:-30 60}"
attempt_logs_dir="$(mktemp -d)"
trap 'rm -rf -- "${attempt_logs_dir}"' EXIT

if ! [[ "${max_attempts}" =~ ^[1-3]$ ]]; then
  echo "EDGEONE_MAX_ATTEMPTS must be an integer from 1 to 3" >&2
  exit 2
fi

read -r -a retry_delays <<< "${retry_delays_text}"
required_delays=$((max_attempts - 1))
if (( ${#retry_delays[@]} < required_delays )); then
  echo "EDGEONE_RETRY_DELAYS_SECONDS needs ${required_delays} delay values" >&2
  exit 2
fi

for delay in "${retry_delays[@]:0:required_delays}"; do
  if ! [[ "${delay}" =~ ^[0-9]{1,3}$ ]] || (( 10#${delay} > 300 )); then
    echo "EDGEONE_RETRY_DELAYS_SECONDS values must be from 0 to 300" >&2
    exit 2
  fi
done

run_deploy() {
  local output_file="$1"
  local -a pipeline_status
  local status
  local tee_status

  set +e
  "${npx_bin}" --yes "edgeone@${cli_version}" makers deploy \
    "${deploy_directory}" \
    -n "${project_name}" \
    -t "${EDGEONE_API_TOKEN}" \
    -e production 2>&1 | tee "${output_file}"
  pipeline_status=("${PIPESTATUS[@]}")
  status="${pipeline_status[0]}"
  tee_status="${pipeline_status[1]}"
  set -e
  attempt_output_ok=1
  if (( tee_status != 0 )); then
    attempt_output_ok=0
  fi
  return "${status}"
}

deployment_started() {
  local pattern
  pattern='Uploading folder to EdgeOne COS|Creating deployment in .* environment'
  pattern+="|Created deployment with Deployment ID|Waiting for deployment to complete"
  pattern+="|Deployment in progress|Deploy Success|Deploy URL:|Console URL:"
  pattern+="|deployment .*accepted|remote state|response body unavailable"
  grep -Eiq "${pattern}" "$1"
}

emit_failure_class() {
  echo "EDGEONE_FAILURE_CLASS=$1" >&2
}

classify_failure() {
  local output_file="$1"
  local auth_pattern
  local project_pattern
  local endpoint_pattern

  if deployment_started "${output_file}"; then
    echo "deployment_ambiguous"
    return
  fi

  project_pattern='PROJECT_[A-Z0-9_]+|project[[:space:]_-]+(not found|does not exist)'
  project_pattern+="|project[[:space:]_-]+permission denied"
  project_pattern+="|permission[[:space:]]+(denied|insufficient)|invalid project"
  project_pattern+="|project .*does not support direct|Provider .*does not support"
  project_pattern+="|failed to (get|retrieve) project"
  project_pattern+="|API request failed \(403\):.*(project|permission|direct|Provider)"
  if grep -Eiq "${project_pattern}" "${output_file}"; then
    echo "project_or_permission"
    return
  fi

  auth_pattern='API request failed \((401|403)\)|\b(Unauthorized|AccessDenied)\b'
  auth_pattern+="|\b(AUTH_[A-Z0-9_]+|TOKEN_[A-Z0-9_]+)\b"
  if grep -Eiq "${auth_pattern}" "${output_file}"; then
    echo "credential_rejected"
    return
  fi

  endpoint_pattern='AxiosError: (timeout|Network Error)|ETIMEDOUT|ECONNRESET'
  endpoint_pattern+="|ECONNREFUSED|ENOTFOUND|socket hang up"
  endpoint_pattern+="|API request failed \(5[0-9]{2}\)|\b522\b"
  if grep -Eiq "${endpoint_pattern}" "${output_file}"; then
    echo "endpoint_unavailable"
    return
  fi

  echo "auth_or_endpoint_ambiguous"
}

normalize_cli_line() {
  local line="$1"
  line="$(printf '%s\n' "${line}" | sed -E $'s/\033\\[[0-9;]*m//g')"
  printf '%s\n' "${line}" | sed -E \
    's/^[[:space:]]*\[cli\][[:space:]]*(\[[^][]*\][[:space:]]*)?//'
}

retryable_preflight_failure() {
  local line
  local normalized_line
  local saw_preflight_context=0
  local saw_transport_failure=0

  while IFS= read -r line || [[ -n "${line}" ]]; do
    normalized_line="$(normalize_cli_line "${line}")"
    if [[ -z "${normalized_line}" ]]; then
      continue
    fi

    if printf '%s\n' "${normalized_line}" | grep -Eiq \
      '^[[:space:]]*npm (warn|notice)[[:space:]]'; then
      continue
    fi

    if (( saw_transport_failure == 1 )); then
      if printf '%s\n' "${normalized_line}" | grep -Eiq \
        '^Deployment failed: Invalid EDGEONE_PAGES_API_TOKEN\.'; then
        continue
      fi
      return 1
    fi

    if printf '%s\n' "${normalized_line}" | grep -Eiq \
      '^[[:space:]]*check and set region failed:[[:space:]]*(AxiosError:[[:space:]]+timeout([[:space:]]+of[[:space:]]+[0-9]+ms[[:space:]]+exceeded)?|AxiosError:[[:space:]]+Network Error|ETIMEDOUT|ECONNRESET|ECONNREFUSED|ENOTFOUND|socket hang up)[[:space:]]*$'; then
      if (( saw_preflight_context == 0 )); then
        return 1
      fi
      saw_transport_failure=1
      continue
    fi

    if printf '%s\n' "${normalized_line}" | grep -Eiq \
      '^[[:space:]]*(Resolving dependencies|Resolved, downloaded and extracted \[[0-9]+\]|Saved lockfile|Using provided API token for deployment\.\.\.|Validating API endpoint\.\.\.)[[:space:]]*$'; then
      saw_preflight_context=1
      continue
    fi

    return 1
  done < "$1"

  if (( saw_preflight_context == 1 && saw_transport_failure == 1 )); then
    return 0
  fi
  return 1
}

for ((attempt = 1; attempt <= max_attempts; attempt++)); do
  attempt_log="${attempt_logs_dir}/attempt-${attempt}.log"
  echo "EdgeOne deployment attempt ${attempt}/${max_attempts} for ${target_label}."
  if run_deploy "${attempt_log}"; then
    status=0
  else
    status=$?
  fi

  if (( attempt_output_ok == 0 )); then
    if (( status == 0 )); then
      echo "EdgeOne CLI succeeded, but output capture failed; retry disabled." >&2
      exit 0
    fi
    if deployment_started "${attempt_log}"; then
      emit_failure_class "deployment_ambiguous"
    else
      emit_failure_class "auth_or_endpoint_ambiguous"
    fi
    echo "EdgeOne CLI output capture failed; automatic retry is disabled." >&2
    exit "${status}"
  fi

  if (( status == 0 )); then
    echo "EdgeOne deployment succeeded for ${target_label}."
    exit 0
  fi

  if deployment_started "${attempt_log}"; then
    emit_failure_class "deployment_ambiguous"
    echo "EdgeOne deployment reached an ambiguous remote state; retry disabled." >&2
    exit "${status}"
  fi

  if ! retryable_preflight_failure "${attempt_log}"; then
    failure_class="$(classify_failure "${attempt_log}")"
    emit_failure_class "${failure_class}"
    echo "EdgeOne deployment failed; automatic retry is disabled." >&2
    if [[ "${failure_class}" == "auth_or_endpoint_ambiguous" ]]; then
      echo "Token invalidity is unproven; inspect endpoint and credential evidence." >&2
    fi
    exit "${status}"
  fi

  if (( attempt == max_attempts )); then
    emit_failure_class "endpoint_unavailable"
    echo "EdgeOne deployment failed after ${max_attempts} preflight attempts." >&2
    exit "${status}"
  fi

  delay="${retry_delays[attempt - 1]}"
  echo "EdgeOne preflight failed; retrying in ${delay}s." >&2
  sleep "${delay}"
done
