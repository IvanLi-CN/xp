#!/usr/bin/env bash
set -euo pipefail

# Run real-xray e2e tests on the shared testbox (codex-testbox).
#
# This follows the shared-testbox-runner rules:
# - per-run isolation under the current shared-testbox Agent Directory
# - unique docker compose project name
# - LXC cap compatibility override
# - safe cleanup (only resources created by this run)

TESTBOX="${TESTBOX:-codex-testbox}"
RUN_MESH_RESOURCE="${XP_RUN_MESH_RESOURCE:-0}"
ONLY_MESH_RESOURCE="${XP_E2E_ONLY_MESH_RESOURCE:-0}"
MESH_RESOURCE_SUMMARY_ONLY="${XP_MESH_RESOURCE_SUMMARY_ONLY:-0}"
if [ "$MESH_RESOURCE_SUMMARY_ONLY" = "1" ]; then
  if [ "$RUN_MESH_RESOURCE" != "1" ]; then
    echo "XP_MESH_RESOURCE_SUMMARY_ONLY=1 requires XP_RUN_MESH_RESOURCE=1" >&2
    exit 2
  fi
  ONLY_MESH_RESOURCE=1
fi
# Compare resource changes with the checked-out development baseline. Older hard-coded
# Mesh baselines can no longer exercise the current signed control-plane protocol.
MESH_RESOURCE_BASELINE_SHA="${XP_MESH_RESOURCE_BASELINE_SHA:-origin/main}"
TESTBOX_CARGO_CACHE_ROOT="${XP_TESTBOX_CARGO_CACHE_ROOT:-}"
TESTBOX_CARGO_HOME="${XP_TESTBOX_CARGO_HOME:-}"
MESH_RESOURCE_CACHE_SMOKE="${XP_TESTBOX_CACHE_SMOKE:-0}"
case "$MESH_RESOURCE_CACHE_SMOKE" in
  0|1) ;;
  *)
    echo "XP_TESTBOX_CACHE_SMOKE must be 0 or 1" >&2
    exit 2
    ;;
esac
if [ "$RUN_MESH_RESOURCE" = "1" ]; then
  if [ -z "$TESTBOX_CARGO_CACHE_ROOT" ] || [ -z "$TESTBOX_CARGO_HOME" ]; then
    echo "XP_TESTBOX_CARGO_CACHE_ROOT and XP_TESTBOX_CARGO_HOME are required for Mesh resource runs" >&2
    exit 2
  fi
  if [ "$MESH_RESOURCE_CACHE_SMOKE" = "1" ]; then
    MESH_RESOURCE_DURATION_SECS="${XP_MESH_RESOURCE_DURATION_SECS:-600}"
    if [ "$MESH_RESOURCE_SUMMARY_ONLY" != "1" ] && [ "$MESH_RESOURCE_DURATION_SECS" != "600" ]; then
      echo "cache smoke requires XP_MESH_RESOURCE_DURATION_SECS=600" >&2
      exit 2
    fi
  else
    MESH_RESOURCE_DURATION_SECS="${XP_MESH_RESOURCE_DURATION_SECS:-900}"
    if [ "$MESH_RESOURCE_SUMMARY_ONLY" != "1" ] && [ "$MESH_RESOURCE_DURATION_SECS" != "900" ]; then
      echo "the final Mesh resource gate requires XP_MESH_RESOURCE_DURATION_SECS=900" >&2
      exit 2
    fi
  fi
else
  MESH_RESOURCE_DURATION_SECS="${XP_MESH_RESOURCE_DURATION_SECS:-900}"
fi

# 1) Identify local repo root (fallback to current dir if not a git repo).
if REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)"; then
  :
else
  REPO_ROOT="$(pwd)"
fi
REPO_ROOT="$(python3 - "$REPO_ROOT" <<'PY'
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
)"

if [ -n "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=all)" ]; then
  echo "testbox requires a clean worktree; commit or discard local changes first" >&2
  exit 2
fi
GIT_SHA_FULL="$(git -C "$REPO_ROOT" rev-parse HEAD)"
if ! command -v bun >/dev/null 2>&1; then
  echo "missing bun; build the candidate Web shell with Bun before running the testbox gate" >&2
  exit 2
fi
(
  cd "$REPO_ROOT/web"
  XP_WEB_BUILD_ID="$GIT_SHA_FULL" bun install --frozen-lockfile
  XP_WEB_BUILD_ID="$GIT_SHA_FULL" bun run build
)
if [ ! -f "$REPO_ROOT/web/dist/index.html" ] || [ ! -f "$REPO_ROOT/web/dist/sw.js" ]; then
  echo "candidate Web shell build did not produce index.html and sw.js" >&2
  exit 2
fi
if ! grep -R -F -- "$GIT_SHA_FULL" "$REPO_ROOT/web/dist" >/dev/null; then
  echo "candidate Web shell does not embed commit $GIT_SHA_FULL" >&2
  exit 2
fi
SOURCE_ARCHIVE="$(mktemp -t xp-testbox-source.XXXXXX.tar)"
WEB_DIST_ARCHIVE="$(mktemp -t xp-testbox-web-dist.XXXXXX.tar)"
BASELINE_ARCHIVE=""
create_deterministic_tar() {
  local source_dir="$1"
  local archive_path="$2"
  python3 - "$source_dir" "$archive_path" <<'PY'
import os
import pathlib
import sys
import tarfile

root = pathlib.Path(sys.argv[1]).resolve()
archive = pathlib.Path(sys.argv[2])
with tarfile.open(archive, "w") as output:
    for path in sorted(root.rglob("*")):
        info = output.gettarinfo(str(path), arcname=str(path.relative_to(root)))
        info.uid = 0
        info.gid = 0
        info.uname = ""
        info.gname = ""
        info.mtime = 0
        if info.isfile():
            with path.open("rb") as source:
                output.addfile(info, source)
        else:
            output.addfile(info)
PY
}
git -C "$REPO_ROOT" archive --format=tar "$GIT_SHA_FULL" > "$SOURCE_ARCHIVE"
SOURCE_ARCHIVE_SHA="$(shasum -a 256 "$SOURCE_ARCHIVE" | awk '{print $1}')"
create_deterministic_tar "$REPO_ROOT/web/dist" "$WEB_DIST_ARCHIVE"
WEB_DIST_ARCHIVE_SHA="$(shasum -a 256 "$WEB_DIST_ARCHIVE" | awk '{print $1}')"

REPO_NAME="$(basename "$REPO_ROOT")"
case "$REPO_NAME" in
  ""|*[!A-Za-z0-9._-]*)
    echo "repository name contains unsupported path characters: $REPO_NAME" >&2
    exit 2
    ;;
esac
PATH_HASH8="$(python3 - "$REPO_ROOT" <<'PY'
import hashlib, os, sys
p=os.path.realpath(sys.argv[1]).encode()
print(hashlib.sha256(p).hexdigest()[:8])
PY
)"

# 2) Per-run identifiers.
GIT_SHA="${GIT_SHA_FULL:0:12}"
RUN_NONCE="$(python3 -c 'import secrets; print(secrets.token_hex(4))')"
RUN_ID="$(date -u +%Y%m%d_%H%M%S)_${GIT_SHA}_${RUN_NONCE}"
WORKSPACE_SLUG="${REPO_NAME}__${PATH_HASH8}"

case "${USER:-}" in
  ""|*[!A-Za-z0-9._-]*)
    echo "USER contains unsupported remote path characters" >&2
    exit 2
    ;;
esac
if [ "$RUN_MESH_RESOURCE" = "1" ]; then
  case "${CODEX_THREAD_ID:-}" in
    ""|*[!A-Za-z0-9._-]*)
      echo "CODEX_THREAD_ID is required for Mesh resource runs" >&2
      exit 2
      ;;
  esac
  REMOTE_AGENT_DIR="/srv/codex/agents/$CODEX_THREAD_ID"
  REMOTE_WORKSPACE="$REMOTE_AGENT_DIR/workspace"
  REMOTE_RUN="$REMOTE_AGENT_DIR/runs/$RUN_ID"
else
  REMOTE_BASE="/srv/codex/workspaces/$USER"
  REMOTE_WORKSPACE="$REMOTE_BASE/$WORKSPACE_SLUG"
  REMOTE_RUN="$REMOTE_WORKSPACE/runs/$RUN_ID"
fi
# Subnet claims are host-global so concurrent runs from different users cannot
# select the same Docker network range.
REMOTE_SUBNET_CLAIMS="/srv/codex/agents/.shared-testbox-subnet-claims"
REMOTE_RESOURCE_BASELINE="$REMOTE_RUN/resource-baseline"

COMPOSE_PROJECT_RAW="codex_${WORKSPACE_SLUG}_${RUN_ID}"
COMPOSE_PROJECT="$(python3 - "$COMPOSE_PROJECT_RAW" <<'PY'
import re, sys
s=sys.argv[1].lower()
s=re.sub(r'[^a-z0-9_-]+','_',s).strip('_')
print(s[:63] if len(s)>63 else s)
PY
)"
REMOTE_RUN_B64="$(printf '%s' "$REMOTE_RUN" | base64 | tr -d '\n')"
COMPOSE_PROJECT_B64="$(printf '%s' "$COMPOSE_PROJECT" | base64 | tr -d '\n')"
SUBNET_CLAIM_ROOT_B64="$(printf '%s' "$REMOTE_SUBNET_CLAIMS" | base64 | tr -d '\n')"
REMOTE_RESOURCE_BASELINE_B64="$(printf '%s' "$REMOTE_RESOURCE_BASELINE" | base64 | tr -d '\n')"
RUN_MESH_RESOURCE_B64="$(printf '%s' "$RUN_MESH_RESOURCE" | base64 | tr -d '\n')"
ONLY_MESH_RESOURCE_B64="$(printf '%s' "$ONLY_MESH_RESOURCE" | base64 | tr -d '\n')"
MESH_RESOURCE_DURATION_B64="$(printf '%s' "$MESH_RESOURCE_DURATION_SECS" | base64 | tr -d '\n')"
MESH_RESOURCE_CACHE_SMOKE_B64="$(printf '%s' "$MESH_RESOURCE_CACHE_SMOKE" | base64 | tr -d '\n')"
MESH_RESOURCE_SUMMARY_ONLY_B64="$(printf '%s' "$MESH_RESOURCE_SUMMARY_ONLY" | base64 | tr -d '\n')"
GIT_SHA_FULL_B64="$(printf '%s' "$GIT_SHA_FULL" | base64 | tr -d '\n')"
RUN_ID_B64="$(printf '%s' "$RUN_ID" | base64 | tr -d '\n')"
REMOTE_WORKSPACE_B64="$(printf '%s' "$REMOTE_WORKSPACE" | base64 | tr -d '\n')"
TESTBOX_CARGO_CACHE_ROOT_B64="$(printf '%s' "$TESTBOX_CARGO_CACHE_ROOT" | base64 | tr -d '\n')"
TESTBOX_CARGO_HOME_B64="$(printf '%s' "$TESTBOX_CARGO_HOME" | base64 | tr -d '\n')"
SOURCE_ARCHIVE_SHA_B64="$(printf '%s' "$SOURCE_ARCHIVE_SHA" | base64 | tr -d '\n')"
WEB_DIST_ARCHIVE_SHA_B64="$(printf '%s' "$WEB_DIST_ARCHIVE_SHA" | base64 | tr -d '\n')"
EVIDENCE_DIR="${XP_TESTBOX_EVIDENCE_DIR:-${TMPDIR:-/tmp}/xp-testbox-evidence}"
EVIDENCE_PATH="$EVIDENCE_DIR/${RUN_ID}.manifest"
EVIDENCE_OUTPUT_PATH="$EVIDENCE_DIR/${RUN_ID}.log"
EVIDENCE_STATUS=running

write_evidence_manifest() {
  local status="${1:-$EVIDENCE_STATUS}"
  /bin/mkdir -p "$EVIDENCE_DIR"
  /bin/chmod 700 "$EVIDENCE_DIR" 2>/dev/null || true
  /usr/bin/printf '%s\n' \
    "run_id=$RUN_ID" \
    "created_utc=$CREATED_UTC" \
    "git_commit=$GIT_SHA_FULL" \
    "source_archive_sha256=$SOURCE_ARCHIVE_SHA" \
    "web_dist_archive_sha256=$WEB_DIST_ARCHIVE_SHA" \
    "baseline_archive_sha256=${BASELINE_ARCHIVE_SHA:-none}" \
    "resource_gate_mode=$([ "$MESH_RESOURCE_CACHE_SMOKE" = "1" ] && printf smoke || printf formal)" \
    "status=$status" \
    "output_log=$EVIDENCE_OUTPUT_PATH" > "$EVIDENCE_PATH"
}

echo "testbox=$TESTBOX"
echo "remote_run=$REMOTE_RUN"
echo "compose_project=$COMPOSE_PROJECT"
echo "evidence_manifest=$EVIDENCE_PATH"

REMOTE_RUN_CREATED=0
cleanup_local() {
  set +e
  write_evidence_manifest "${EVIDENCE_STATUS:-interrupted}"
  rm -f "$SOURCE_ARCHIVE" "$WEB_DIST_ARCHIVE"
  if [ -n "${BASELINE_ARCHIVE:-}" ]; then
    rm -f "$BASELINE_ARCHIVE"
  fi
  if [ "${REMOTE_RUN_CREATED:-0}" = "1" ]; then
    ssh -o BatchMode=yes "$TESTBOX" \
      "REMOTE_RUN_B64='$REMOTE_RUN_B64' COMPOSE_PROJECT_B64='$COMPOSE_PROJECT_B64' RUN_ID_B64='$RUN_ID_B64' bash -s" <<'REMOTE_CLEANUP' >/dev/null 2>&1 || true
set -euo pipefail
REMOTE_RUN="$(printf '%s' "${REMOTE_RUN_B64:?}" | base64 -d)"
COMPOSE_PROJECT="$(printf '%s' "${COMPOSE_PROJECT_B64:?}" | base64 -d)"
RUN_ID="$(printf '%s' "${RUN_ID_B64:?}" | base64 -d)"
case "$REMOTE_RUN" in
  /srv/codex/agents/*/runs/*|/srv/codex/workspaces/*/runs/*) ;;
  *) exit 2 ;;
esac
scope_unit="codex-xp-source-journal-${RUN_ID}.scope"
systemctl --user stop "$scope_unit" >/dev/null 2>&1 || true
systemctl --user reset-failed "$scope_unit" >/dev/null 2>&1 || true
for resource_label in candidate-smoke baseline candidate source-journal summary; do
  resource_unit="codex-xp-resource-${RUN_ID}-${resource_label}.scope"
  systemctl --user stop "$resource_unit" >/dev/null 2>&1 || true
  systemctl --user reset-failed "$resource_unit" >/dev/null 2>&1 || true
done
if [ -d "$REMOTE_RUN/scripts/e2e" ]; then
  cd "$REMOTE_RUN/scripts/e2e"
  cleanup_files=()
  for file in docker-compose.xray.yml .codex.caps-compat.yaml .codex.net-compat.yaml .codex.user-compat.yaml; do
    if [ -f "$file" ]; then
      cleanup_files+=(-f "$file")
    fi
  done
  if [ "${#cleanup_files[@]}" -gt 0 ]; then
    docker compose -p "$COMPOSE_PROJECT" "${cleanup_files[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
fi
rm -rf "$REMOTE_RUN"
REMOTE_CLEANUP
  fi
}
on_local_signal() {
  trap - EXIT INT TERM
  cleanup_local
  exit "$1"
}
trap cleanup_local EXIT
trap 'on_local_signal 130' INT
trap 'on_local_signal 143' TERM

# 3) Create remote run dir and attach minimal metadata. Paths and contents are
# transported as base64 so the bootstrap shell never interpolates user input.
CREATED_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
WORKSPACE_METADATA="local_repo_root=$REPO_ROOT
created_utc=$CREATED_UTC
git_commit=$GIT_SHA_FULL
source_archive_sha256=$SOURCE_ARCHIVE_SHA
web_dist_archive_sha256=$WEB_DIST_ARCHIVE_SHA
"
WORKSPACE_METADATA_B64="$(printf '%s' "$WORKSPACE_METADATA" | base64 | tr -d '\n')"
REMOTE_RUN_CREATED=1
ssh -o BatchMode=yes "$TESTBOX" \
  "REMOTE_RUN_B64='$REMOTE_RUN_B64' REMOTE_WORKSPACE_B64='$REMOTE_WORKSPACE_B64' WORKSPACE_METADATA_B64='$WORKSPACE_METADATA_B64' bash -s" <<'REMOTE_BOOTSTRAP'
set -euo pipefail
REMOTE_RUN="$(printf '%s' "${REMOTE_RUN_B64:?}" | base64 -d)"
REMOTE_WORKSPACE="$(printf '%s' "${REMOTE_WORKSPACE_B64:?}" | base64 -d)"
if [[ "$REMOTE_RUN" == /srv/codex/agents/*/runs/* ]]; then
  agent_base=/srv/codex/agents
  agent_dir="$(dirname "$REMOTE_WORKSPACE")"
  [[ -d "$agent_base" && ! -L "$agent_base" ]]
  [[ "$(readlink -f -- "$agent_base")" == "$agent_base" ]]
  if [[ -e "$agent_dir" || -L "$agent_dir" ]]; then
    [[ -d "$agent_dir" && ! -L "$agent_dir" ]]
  else
    mkdir -- "$agent_dir"
  fi
  [[ "$(readlink -f -- "$agent_dir")" == "$agent_dir" ]]
fi
mkdir -p "$REMOTE_WORKSPACE" "$REMOTE_RUN"
printf '%s' "${WORKSPACE_METADATA_B64:?}" | base64 -d > "$REMOTE_WORKSPACE/workspace.txt"
REMOTE_BOOTSTRAP

# 4) Sync the immutable tracked tree, then overlay the generated Web shell.
rsync -a "$SOURCE_ARCHIVE" "$TESTBOX:$REMOTE_RUN/source.tar"
ssh -o BatchMode=yes "$TESTBOX" \
  "test \"\$(sha256sum '$REMOTE_RUN/source.tar' | awk '{print \$1}')\" = '$SOURCE_ARCHIVE_SHA' && mkdir -p '$REMOTE_RUN/candidate-source' && tar -xf '$REMOTE_RUN/source.tar' -C '$REMOTE_RUN' && tar -xf '$REMOTE_RUN/source.tar' -C '$REMOTE_RUN/candidate-source' && rm -f '$REMOTE_RUN/source.tar'"
rsync -a "$WEB_DIST_ARCHIVE" "$TESTBOX:$REMOTE_RUN/web-dist.tar"
ssh -o BatchMode=yes "$TESTBOX" \
  "test \"\$(sha256sum '$REMOTE_RUN/web-dist.tar' | awk '{print \$1}')\" = '$WEB_DIST_ARCHIVE_SHA' && mkdir -p '$REMOTE_RUN/web/dist' '$REMOTE_RUN/candidate-source/web/dist' && tar -xf '$REMOTE_RUN/web-dist.tar' -C '$REMOTE_RUN/web/dist' && tar -xf '$REMOTE_RUN/web-dist.tar' -C '$REMOTE_RUN/candidate-source/web/dist' && rm -f '$REMOTE_RUN/web-dist.tar'"

if [ "$RUN_MESH_RESOURCE" = "1" ] && [ "$MESH_RESOURCE_SUMMARY_ONLY" != "1" ]; then
  git -C "$REPO_ROOT" cat-file -e "$MESH_RESOURCE_BASELINE_SHA^{commit}"
  BASELINE_ARCHIVE="$(mktemp -t xp-testbox-baseline.XXXXXX.tar)"
  git -C "$REPO_ROOT" archive --format=tar "$MESH_RESOURCE_BASELINE_SHA" > "$BASELINE_ARCHIVE"
  BASELINE_ARCHIVE_SHA="$(shasum -a 256 "$BASELINE_ARCHIVE" | awk '{print $1}')"
  rsync -a "$BASELINE_ARCHIVE" "$TESTBOX:$REMOTE_RUN/resource-baseline.tar"
  ssh -o BatchMode=yes "$TESTBOX" \
    "test \"\$(sha256sum '$REMOTE_RUN/resource-baseline.tar' | awk '{print \$1}')\" = '$BASELINE_ARCHIVE_SHA' && mkdir -p '$REMOTE_RESOURCE_BASELINE' && tar -xf '$REMOTE_RUN/resource-baseline.tar' -C '$REMOTE_RESOURCE_BASELINE' && rm -f '$REMOTE_RUN/resource-baseline.tar'"
  rsync -az --delete "$REPO_ROOT/web/dist/" "$TESTBOX:$REMOTE_RESOURCE_BASELINE/web/dist/"
fi
BASELINE_ARCHIVE_SHA_B64="$(printf '%s' "${BASELINE_ARCHIVE_SHA:-none}" | base64 | tr -d '\n')"

# 5) Run on testbox.
if ssh -o BatchMode=yes "$TESTBOX" \
  "REMOTE_RUN_B64='$REMOTE_RUN_B64' REMOTE_WORKSPACE_B64='$REMOTE_WORKSPACE_B64' COMPOSE_PROJECT_B64='$COMPOSE_PROJECT_B64' SUBNET_CLAIM_ROOT_B64='$SUBNET_CLAIM_ROOT_B64' REMOTE_RESOURCE_BASELINE_B64='$REMOTE_RESOURCE_BASELINE_B64' RUN_MESH_RESOURCE_B64='$RUN_MESH_RESOURCE_B64' ONLY_MESH_RESOURCE_B64='$ONLY_MESH_RESOURCE_B64' MESH_RESOURCE_DURATION_B64='$MESH_RESOURCE_DURATION_B64' MESH_RESOURCE_CACHE_SMOKE_B64='$MESH_RESOURCE_CACHE_SMOKE_B64' MESH_RESOURCE_SUMMARY_ONLY_B64='$MESH_RESOURCE_SUMMARY_ONLY_B64' GIT_SHA_FULL_B64='$GIT_SHA_FULL_B64' RUN_ID_B64='$RUN_ID_B64' TESTBOX_CARGO_CACHE_ROOT_B64='$TESTBOX_CARGO_CACHE_ROOT_B64' TESTBOX_CARGO_HOME_B64='$TESTBOX_CARGO_HOME_B64' SOURCE_ARCHIVE_SHA_B64='$SOURCE_ARCHIVE_SHA_B64' WEB_DIST_ARCHIVE_SHA_B64='$WEB_DIST_ARCHIVE_SHA_B64' BASELINE_ARCHIVE_SHA_B64='$BASELINE_ARCHIVE_SHA_B64' bash -s" 2>&1 <<'REMOTE' | tee "$EVIDENCE_OUTPUT_PATH"
set -euo pipefail

REMOTE_RUN="$(printf '%s' "${REMOTE_RUN_B64:?}" | base64 -d)"
COMPOSE_PROJECT="$(printf '%s' "${COMPOSE_PROJECT_B64:?}" | base64 -d)"
SUBNET_CLAIM_ROOT="$(printf '%s' "${SUBNET_CLAIM_ROOT_B64:?}" | base64 -d)"
REMOTE_RESOURCE_BASELINE="$(printf '%s' "${REMOTE_RESOURCE_BASELINE_B64:?}" | base64 -d)"
REMOTE_WORKSPACE="$(printf '%s' "${REMOTE_WORKSPACE_B64:?}" | base64 -d)"
RUN_MESH_RESOURCE="$(printf '%s' "${RUN_MESH_RESOURCE_B64:?}" | base64 -d)"
ONLY_MESH_RESOURCE="$(printf '%s' "${ONLY_MESH_RESOURCE_B64:?}" | base64 -d)"
MESH_RESOURCE_DURATION="$(printf '%s' "${MESH_RESOURCE_DURATION_B64:?}" | base64 -d)"
MESH_RESOURCE_CACHE_SMOKE="$(printf '%s' "${MESH_RESOURCE_CACHE_SMOKE_B64:?}" | base64 -d)"
MESH_RESOURCE_SUMMARY_ONLY="$(printf '%s' "${MESH_RESOURCE_SUMMARY_ONLY_B64:?}" | base64 -d)"
GIT_SHA_FULL="$(printf '%s' "${GIT_SHA_FULL_B64:?}" | base64 -d)"
RUN_ID="$(printf '%s' "${RUN_ID_B64:?}" | base64 -d)"
TESTBOX_CARGO_CACHE_ROOT="$(printf '%s' "${TESTBOX_CARGO_CACHE_ROOT_B64:?}" | base64 -d)"
TESTBOX_CARGO_HOME="$(printf '%s' "${TESTBOX_CARGO_HOME_B64:?}" | base64 -d)"
SOURCE_ARCHIVE_SHA="$(printf '%s' "${SOURCE_ARCHIVE_SHA_B64:?}" | base64 -d)"
WEB_DIST_ARCHIVE_SHA="$(printf '%s' "${WEB_DIST_ARCHIVE_SHA_B64:?}" | base64 -d)"
BASELINE_ARCHIVE_SHA="$(printf '%s' "${BASELINE_ARCHIVE_SHA_B64:?}" | base64 -d)"

cleanup() {
  if [ "${CLEANUP_DONE:-0}" = "1" ]; then
    return
  fi
  CLEANUP_DONE=1
  set +e
  if [ -n "${REMOTE_RUN:-}" ] && [ -d "$REMOTE_RUN/scripts/e2e" ]; then
    cd "$REMOTE_RUN/scripts/e2e" || return 0
    if [ -f "docker-compose.xray.yml" ] && [ -f ".codex.caps-compat.yaml" ] && [ -f ".codex.net-compat.yaml" ]; then
      cleanup_files=(-f "docker-compose.xray.yml" -f ".codex.caps-compat.yaml" -f ".codex.net-compat.yaml")
      if [ -f ".codex.user-compat.yaml" ]; then
        cleanup_files+=(-f ".codex.user-compat.yaml")
      fi
      docker compose -p "$COMPOSE_PROJECT" "${cleanup_files[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
    fi
  fi
  if [ -n "${SUBNET_CLAIM_DIR:-}" ] && [ -d "$SUBNET_CLAIM_DIR" ]; then
    rm -rf "$SUBNET_CLAIM_DIR" >/dev/null 2>&1 || true
  fi
  if [ -n "${REMOTE_RUN:-}" ]; then
    rm -rf "$REMOTE_RUN" >/dev/null 2>&1 || true
  fi
}
on_signal() {
  cleanup
  trap - EXIT INT TERM
  exit "$1"
}
trap cleanup EXIT
trap 'on_signal 130' INT
trap 'on_signal 143' TERM

if [ "$RUN_MESH_RESOURCE" = "1" ]; then
  case "$TESTBOX_CARGO_CACHE_ROOT:$TESTBOX_CARGO_HOME" in
    /*:/*) ;;
    *)
      echo "Cargo cache paths must be absolute" >&2
      exit 2
      ;;
  esac
  case "$TESTBOX_CARGO_CACHE_ROOT" in
    /|*/..|*/../*)
      echo "Cargo cache root must be a dedicated directory" >&2
      exit 2
      ;;
  esac
  if [ "$TESTBOX_CARGO_CACHE_ROOT" = "$TESTBOX_CARGO_HOME" ]; then
    echo "Cargo cache root and Cargo Home must be separate" >&2
    exit 2
  fi
  if [ -L "$TESTBOX_CARGO_CACHE_ROOT" ]; then
    echo "Cargo cache root must not be a symlink" >&2
    exit 2
  fi
  for managed_path in \
    "$TESTBOX_CARGO_CACHE_ROOT/source" \
    "$TESTBOX_CARGO_CACHE_ROOT/target"; do
    if [ -L "$managed_path" ]; then
      echo "managed Cargo cache path must not be a symlink: $managed_path" >&2
      exit 2
    fi
  done
  agent_root="$(dirname "$REMOTE_WORKSPACE")"
  agent_root_real="$(readlink -f -- "$agent_root")"
  [ -n "$agent_root_real" ] || exit 2
  case "$agent_root_real" in
    /srv/codex/agents/*) ;;
    *)
      echo "remote workspace is not inside the shared-testbox Agent Directory" >&2
      exit 2
      ;;
  esac
  assert_no_symlink_path() {
    local path="$1"
    while [ "$path" != "/" ] && [ "$path" != "." ]; do
      if [ -L "$path" ]; then
        echo "cache path contains a symlink: $path" >&2
        exit 2
      fi
      path="$(dirname "$path")"
    done
  }
  assert_no_symlink_path "$agent_root"
  assert_no_symlink_path "$TESTBOX_CARGO_CACHE_ROOT"
  assert_no_symlink_path "$TESTBOX_CARGO_HOME"
  mkdir -p "$TESTBOX_CARGO_CACHE_ROOT/source"
  cache_root_real="$(readlink -f -- "$TESTBOX_CARGO_CACHE_ROOT")"
  case "$cache_root_real" in
    "$agent_root_real"/*) ;;
    *)
      echo "Cargo cache root must be inside the current Agent Directory" >&2
      exit 2
      ;;
  esac
  cargo_home_parent_real="$(readlink -f -- "$(dirname "$TESTBOX_CARGO_HOME")")"
  case "$TESTBOX_CARGO_HOME" in
    /srv/codex/caches/linux-amd64/cargo) ;;
    *)
      echo "Cargo Home must be /srv/codex/caches/linux-amd64/cargo" >&2
      exit 2
      ;;
  esac
  case "$cargo_home_parent_real" in
    /srv/codex/caches/linux-amd64) ;;
    *)
      echo "Cargo Home must be inside the shared Cargo cache root" >&2
      exit 2
      ;;
  esac
  command -v flock >/dev/null 2>&1 || {
    echo "flock is required on the shared testbox" >&2
    exit 2
  }
  if [ -L "$TESTBOX_CARGO_CACHE_ROOT/.xp-resource-run.lock" ]; then
    echo "resource cache lock must not be a symlink" >&2
    exit 2
  fi
  exec 8>"$TESTBOX_CARGO_CACHE_ROOT/.xp-resource-run.lock"
  flock -x 8
  toolchain_key="$(rustc -Vv | sha256sum | awk '{print substr($1, 1, 16)}')"
  candidate_resource_source="$TESTBOX_CARGO_CACHE_ROOT/source/candidate"
  baseline_resource_source="$TESTBOX_CARGO_CACHE_ROOT/source/baseline"
  candidate_resource_target="$TESTBOX_CARGO_CACHE_ROOT/target/$toolchain_key/candidate"
  baseline_resource_target="$TESTBOX_CARGO_CACHE_ROOT/target/$toolchain_key/baseline"
  for managed_path in \
    "$candidate_resource_source" \
    "$baseline_resource_source" \
    "$candidate_resource_target" \
    "$baseline_resource_target"; do
    if [ -L "$managed_path" ]; then
      echo "managed Cargo cache path must not be a symlink: $managed_path" >&2
      exit 2
    fi
  done

  sync_cache_source() {
    local source_dir="$1"
    local destination="$2"
    local marker="$3"
    local slot="$4"
    local source_root="$TESTBOX_CARGO_CACHE_ROOT/source"
    local staging="$source_root/.incoming-${slot}.$$"
    local marker_file="$destination/.xp-source-marker"

    case "$destination" in
      "$source_root"/*) ;;
      *)
        echo "cache source escaped the cache root" >&2
        exit 2
        ;;
    esac
    if [ -L "$destination" ] || [ -L "$staging" ]; then
      echo "cache source path must not be a symlink" >&2
      exit 2
    fi
    if [ -f "$marker_file" ] && [ "$(cat "$marker_file")" = "$marker" ] &&
      [ -f "$destination/Cargo.toml" ]; then
      echo "cargo_cache_source=hit slot=$slot"
      return
    fi
    rm -rf "$staging"
    mkdir -p "$staging"
    cp -a "$source_dir/." "$staging/"
    printf '%s\n' "$marker" > "$staging/.xp-source-marker"
    rm -rf "$destination"
    mv "$staging" "$destination"
    echo "cargo_cache_source=refreshed slot=$slot"
  }
fi

cd "$REMOTE_RUN/scripts/e2e"

if ! command -v make >/dev/null 2>&1; then
  echo "missing 'make' on codex-testbox; vendored OpenSSL builds require it" >&2
  echo "repair with shared-testbox-bootstrap before running real-Xray suites" >&2
  exit 2
fi

# Random host ports (avoid collisions).
XP_E2E_XRAY_API_PORT="$(python3 - <<'PY'
import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()
PY
)"
XP_E2E_SS_PORT="$(python3 - <<'PY'
import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()
PY
)"
XP_E2E_VLESS_PORT="$(python3 - <<'PY'
import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()
PY
)"
while [ "$XP_E2E_SS_PORT" = "$XP_E2E_XRAY_API_PORT" ]; do
  XP_E2E_SS_PORT="$(python3 - <<'PY'
import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()
PY
  )"
done
while [ "$XP_E2E_VLESS_PORT" = "$XP_E2E_XRAY_API_PORT" ] || [ "$XP_E2E_VLESS_PORT" = "$XP_E2E_SS_PORT" ]; do
  XP_E2E_VLESS_PORT="$(python3 - <<'PY'
import socket
s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()
PY
  )"
done
export XP_E2E_XRAY_API_PORT XP_E2E_SS_PORT XP_E2E_VLESS_PORT

COMPOSE_FILE="docker-compose.xray.yml"
SUBNET_CLAIM_DIR=""

# LXC quirk: CAP_SETFCAP is not available. Default Docker caps include it.
# Workaround: drop ALL caps, then add back a known-good set (default minus SETFCAP).
caps_override=".codex.caps-compat.yaml"
net_override=".codex.net-compat.yaml"
user_override=".codex.user-compat.yaml"
services="$(docker compose -f "$COMPOSE_FILE" config --services)"
{
  echo "services:"
  for s in $services; do
    cat <<YAML
  $s:
    cap_drop:
      - ALL
    cap_add:
      - CHOWN
      - DAC_OVERRIDE
      - FSETID
      - FOWNER
      - MKNOD
      - NET_RAW
      - SETGID
      - SETUID
      - SETPCAP
      - NET_BIND_SERVICE
      - SYS_CHROOT
      - KILL
      - AUDIT_WRITE
YAML
  done
} > "$caps_override"

mapfile -t subnet_claim_info < <(
  python3 - "$SUBNET_CLAIM_ROOT" "$REMOTE_RUN" "$COMPOSE_PROJECT" <<'PY'
import ipaddress
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

claim_root = pathlib.Path(sys.argv[1])
remote_run = pathlib.Path(sys.argv[2])
compose_project = sys.argv[3]
lock_dir = claim_root / ".allocator.lock"
lock_timeout_seconds = 30
lock_poll_seconds = 0.2
pending_claim_grace_seconds = 120

claim_root.mkdir(parents=True, exist_ok=True)

deadline = time.time() + lock_timeout_seconds
while True:
    try:
        os.mkdir(lock_dir)
        break
    except FileExistsError:
        if time.time() >= deadline:
            print("timed out waiting for shared-testbox subnet allocator lock", file=sys.stderr)
            sys.exit(1)
        time.sleep(lock_poll_seconds)

used = []
claimed = []

try:
    try:
        docker_ids = subprocess.check_output(
            ["docker", "network", "ls", "-q"],
            text=True,
        ).split()
    except (FileNotFoundError, subprocess.CalledProcessError) as exc:
        print(f"cannot enumerate Docker networks for subnet allocation: {exc}", file=sys.stderr)
        sys.exit(1)

    if docker_ids:
        inspect = json.loads(
            subprocess.check_output(["docker", "network", "inspect", *docker_ids], text=True)
        )
        for network in inspect:
            for cfg in (network.get("IPAM") or {}).get("Config") or []:
                subnet = cfg.get("Subnet")
                if subnet:
                    try:
                        used.append(ipaddress.ip_network(subnet, strict=False))
                    except ValueError:
                        pass

    for line in subprocess.check_output(
        ["ip", "-o", "-4", "addr", "show"],
        text=True,
    ).splitlines():
        parts = line.split()
        if len(parts) >= 4:
            try:
                used.append(ipaddress.ip_network(parts[3], strict=False))
            except ValueError:
                pass

    for claim_dir in claim_root.iterdir():
        if not claim_dir.is_dir() or claim_dir.name.startswith("."):
            continue

        run_path_file = claim_dir / "run_path"
        subnet_file = claim_dir / "subnet"
        if not run_path_file.exists() or not subnet_file.exists():
            shutil.rmtree(claim_dir, ignore_errors=True)
            continue

        run_path = pathlib.Path(run_path_file.read_text().strip())

        def remove_stale_run():
            if run_path.is_symlink() or not run_path.is_dir():
                return
            run_real = run_path.resolve(strict=False)
            allowed_roots = (
                pathlib.Path("/srv/codex/agents").resolve(),
                pathlib.Path("/srv/codex/workspaces").resolve(),
            )
            if not any(
                run_real == root or root in run_real.parents for root in allowed_roots
            ):
                return
            shutil.rmtree(run_path, ignore_errors=True)

        active = False
        lease_file = claim_dir / "lease"
        if run_path.is_dir() and not run_path.is_symlink() and lease_file.is_file():
            lease = {}
            try:
                for line in lease_file.read_text().splitlines():
                    key, value = line.split("=", 1)
                    lease[key] = value
                pid = int(lease["pid"])
                start_ticks = int(lease["start_ticks"])
                proc_stat = pathlib.Path(f"/proc/{pid}/stat").read_text()
                proc_start_ticks = int(proc_stat.rsplit(")", 1)[1].split()[19])
                proc_cwd = pathlib.Path(os.path.realpath(f"/proc/{pid}/cwd"))
                run_real = pathlib.Path(os.path.realpath(run_path))
                active = proc_start_ticks == start_ticks and (
                    proc_cwd == run_real or run_real in proc_cwd.parents
                )
            except (KeyError, ValueError, FileNotFoundError, OSError):
                active = False
        elif run_path.is_dir() and not run_path.is_symlink():
            try:
                active = time.time() - claim_dir.stat().st_mtime < pending_claim_grace_seconds
            except OSError:
                active = False

        if not active:
            remove_stale_run()
            shutil.rmtree(claim_dir, ignore_errors=True)
            continue

        try:
            claimed.append(ipaddress.ip_network(subnet_file.read_text().strip(), strict=False))
        except ValueError:
            shutil.rmtree(claim_dir, ignore_errors=True)

    for octet in range(0, 256):
        candidate = ipaddress.ip_network(f"10.203.{octet}.0/24")
        if any(candidate.overlaps(existing) for existing in used):
            continue
        if any(candidate.overlaps(existing) for existing in claimed):
            continue

        claim_name = f"{candidate.network_address.exploded.replace('.', '_')}_{candidate.prefixlen}"
        claim_dir = claim_root / claim_name
        if claim_dir.exists():
            continue

        claim_dir.mkdir()
        (claim_dir / "run_path").write_text(f"{remote_run}\n")
        (claim_dir / "compose_project").write_text(f"{compose_project}\n")
        (claim_dir / "subnet").write_text(f"{candidate}\n")
        print(candidate)
        print(claim_dir)
        sys.exit(0)

    print("failed to find free subnet for shared testbox compose run", file=sys.stderr)
    sys.exit(1)
finally:
    try:
        os.rmdir(lock_dir)
    except FileNotFoundError:
        pass
PY
)

TESTBOX_SUBNET="${subnet_claim_info[0]:-}"
SUBNET_CLAIM_DIR="${subnet_claim_info[1]:-}"
if [ -z "$TESTBOX_SUBNET" ] || [ -z "$SUBNET_CLAIM_DIR" ]; then
  echo "failed to allocate isolated shared-testbox subnet claim" >&2
  exit 1
fi

claim_start_ticks="$(awk '{print $22}' /proc/$$/stat 2>/dev/null || true)"
if [ -n "$claim_start_ticks" ]; then
  printf 'pid=%s\nstart_ticks=%s\n' "$$" "$claim_start_ticks" > "$SUBNET_CLAIM_DIR/lease"
fi

cat > "$net_override" <<YAML
networks:
  default:
    ipam:
      config:
        - subnet: ${TESTBOX_SUBNET}
YAML

echo "selected subnet: $TESTBOX_SUBNET"
echo "starting xray: api_port=$XP_E2E_XRAY_API_PORT ss_port=$XP_E2E_SS_PORT vless_port=$XP_E2E_VLESS_PORT"
compose_files=(-f "$COMPOSE_FILE" -f "$caps_override" -f "$net_override")
if [ "$RUN_MESH_RESOURCE" = "1" ]; then
  cat > "$user_override" <<YAML
services:
  xray:
    user: "$(id -u):$(id -g)"
YAML
  compose_files+=(-f "$user_override")
fi
docker compose -p "$COMPOSE_PROJECT" "${compose_files[@]}" up -d

echo "waiting for xray gRPC on 127.0.0.1:$XP_E2E_XRAY_API_PORT..."
python3 - <<'PY'
import socket, time, os, sys
host="127.0.0.1"
port=int(os.environ["XP_E2E_XRAY_API_PORT"])
deadline=time.time()+10
while time.time()<deadline:
  s=socket.socket(); s.settimeout(0.2)
  try:
    s.connect((host, port))
    sys.exit(0)
  except OSError:
    time.sleep(0.1)
  finally:
    s.close()
print("xray did not become ready in time", file=sys.stderr)
sys.exit(1)
PY

cd "$REMOTE_RUN"

export RUST_TEST_THREADS=1
export XP_E2E_XRAY_MODE=external
export XP_E2E_XRAY_API_ADDR="127.0.0.1:$XP_E2E_XRAY_API_PORT"
if [ "$ONLY_MESH_RESOURCE" != "1" ]; then
  XP_E2E_MIHOMO_BIN="$("$REMOTE_RUN/scripts/e2e/install-mihomo-v1.19.29.sh")"
  export XP_E2E_MIHOMO_BIN
fi

if [ "$ONLY_MESH_RESOURCE" != "1" ]; then
  cargo test --test xray_e2e -- --ignored
  cargo test --test xray_mesh_transport_e2e -- --ignored
  cargo test --test xray_vless_xhttp_e2e -- --ignored
  cargo test --test shared_quota_xray_e2e -- --ignored
fi

if [ "$RUN_MESH_RESOURCE" = "1" ]; then
  mkdir -p "$REMOTE_RESOURCE_BASELINE/web"
  rm -rf "$REMOTE_RESOURCE_BASELINE/web/dist"
  cp -a "$REMOTE_RUN/web/dist" "$REMOTE_RESOURCE_BASELINE/web/dist"
  sync_cache_source \
    "$REMOTE_RUN/candidate-source" \
    "$candidate_resource_source" \
    "source_archive_sha256=$SOURCE_ARCHIVE_SHA web_dist_archive_sha256=$WEB_DIST_ARCHIVE_SHA build_version=$GIT_SHA_FULL" \
    candidate
  if [ "$MESH_RESOURCE_SUMMARY_ONLY" != "1" ]; then
    sync_cache_source \
      "$REMOTE_RESOURCE_BASELINE" \
      "$baseline_resource_source" \
      "source_archive_sha256=$BASELINE_ARCHIVE_SHA web_dist_archive_sha256=$WEB_DIST_ARCHIVE_SHA build_version=package" \
      baseline
  fi
  cache_tool="$REMOTE_RUN/scripts/cargo-cache/with-cargo-target.sh"
  cargo_phase_started=$SECONDS
  XP_BUILD_VERSION="$GIT_SHA_FULL" \
    "$cache_tool" \
    --cache-root "$TESTBOX_CARGO_CACHE_ROOT" \
    --cargo-home "$TESTBOX_CARGO_HOME" \
    --slot candidate \
    --source "$candidate_resource_source" \
    -- cargo build --release --locked --bin xp
  echo "cargo_phase=resource_candidate_build duration_secs=$((SECONDS - cargo_phase_started))"
  cp "$candidate_resource_target/release/xp" "$REMOTE_RUN/xp-resource-candidate"
  if [ "$MESH_RESOURCE_SUMMARY_ONLY" != "1" ]; then
    cargo_phase_started=$SECONDS
    "$cache_tool" \
      --cache-root "$TESTBOX_CARGO_CACHE_ROOT" \
      --cargo-home "$TESTBOX_CARGO_HOME" \
      --slot baseline \
      --source "$baseline_resource_source" \
      -- cargo build --release --locked --bin xp
    echo "cargo_phase=resource_baseline_build duration_secs=$((SECONDS - cargo_phase_started))"
    cp "$baseline_resource_target/release/xp" "$REMOTE_RUN/xp-resource-baseline"
  fi
  xray_container="$(docker ps -q \
    --filter "label=com.docker.compose.project=$COMPOSE_PROJECT" \
    --filter "label=com.docker.compose.service=xray")"
  if [ -z "$xray_container" ]; then
    echo "resource workload Xray container is unavailable" >&2
    exit 1
  fi
  xray_pid="$(docker inspect -f '{{.State.Pid}}' "$xray_container")"
  if ! command -v systemd-run >/dev/null 2>&1; then
    echo "missing systemd-run; cannot enforce the 128 MiB/no-swap XP resource gate" >&2
    exit 2
  fi
  cargo_phase_started=$SECONDS
  resource_test_bin="$(
    XP_BUILD_VERSION="$GIT_SHA_FULL" \
      "$cache_tool" \
      --cache-root "$TESTBOX_CARGO_CACHE_ROOT" \
      --cargo-home "$TESTBOX_CARGO_HOME" \
      --slot candidate \
      --source "$candidate_resource_source" \
      -- cargo test --release --locked --test mesh_transport_resource_e2e --no-run \
        --message-format=json |
      python3 -c '
import json
import sys

executables = []
for line in sys.stdin:
    message = json.loads(line)
    target = message.get("target", {})
    executable = message.get("executable")
    if (
        message.get("reason") == "compiler-artifact"
        and target.get("name") == "mesh_transport_resource_e2e"
        and executable
    ):
        executables.append(executable)

if len(executables) != 1:
    raise SystemExit(
        "expected one mesh_transport_resource_e2e executable, found "
        f"{len(executables)}"
    )
print(executables[0])
'
  )"
  echo "cargo_phase=resource_test_build duration_secs=$((SECONDS - cargo_phase_started))"
  if [ -z "$resource_test_bin" ]; then
    echo "resource workload test binary was not built" >&2
    exit 1
  fi
  case "$resource_test_bin" in
    "$candidate_resource_target"/*) ;;
    *)
      echo "resource workload test binary escaped the candidate target directory" >&2
      exit 1
      ;;
  esac
  cp "$resource_test_bin" "$REMOTE_RUN/mesh_transport_resource_e2e"
  resource_test_bin="$REMOTE_RUN/mesh_transport_resource_e2e"
  if [ "$MESH_RESOURCE_SUMMARY_ONLY" = "1" ]; then
    echo "running source journal resource workload in the actual XP process (XP memory=128MiB, swap=0)"
    env \
      XP_MESH_RESOURCE_MODE=shared-testbox \
      XP_MESH_RESOURCE_CHILD_CGROUP=1 \
      XP_MESH_RESOURCE_RUN_ID="$RUN_ID" \
      XP_MESH_RESOURCE_CANDIDATE_BIN="$REMOTE_RUN/xp-resource-candidate" \
      XP_MESH_RESOURCE_EXPECT_MEMORY_LIMIT=128MiB \
      "$resource_test_bin" xp_source_delivery_journal_memory_e2e --ignored --nocapture
    echo "running repository summary resource workload (XP memory=128MiB, swap=0)"
    env \
      XP_MESH_RESOURCE_MODE=shared-testbox \
      XP_MESH_RESOURCE_SUMMARY_ONLY=1 \
      XP_MESH_RESOURCE_CHILD_CGROUP=1 \
      XP_MESH_RESOURCE_RUN_ID="$RUN_ID" \
      XP_MESH_RESOURCE_CANDIDATE_BIN="$REMOTE_RUN/xp-resource-candidate" \
      XP_MESH_RESOURCE_EXPECT_MEMORY_LIMIT=128MiB \
      "$resource_test_bin" xp_repository_summary_memory_e2e --ignored --nocapture
  else
    echo "running 50-peer resource workload for ${MESH_RESOURCE_DURATION}s (xray_pid=$xray_pid, XP memory=128MiB, swap=0)"
    env \
      XP_MESH_RESOURCE_MODE=shared-testbox \
      XP_MESH_RESOURCE_CHILD_CGROUP=1 \
      XP_MESH_RESOURCE_RUN_ID="$RUN_ID" \
      XP_MESH_RESOURCE_BASELINE_BIN="$REMOTE_RUN/xp-resource-baseline" \
      XP_MESH_RESOURCE_CANDIDATE_BIN="$REMOTE_RUN/xp-resource-candidate" \
      XP_MESH_RESOURCE_SUPPORT_PIDS="$xray_pid" \
      XP_MESH_RESOURCE_DURATION_SECS="$MESH_RESOURCE_DURATION" \
      XP_MESH_RESOURCE_EXPECT_MEMORY_LIMIT=128MiB \
      "$resource_test_bin" --ignored --nocapture
  fi
fi
REMOTE
then
  EVIDENCE_STATUS=passed
  REMOTE_RUN_CREATED=0
else
  status=${PIPESTATUS[0]}
  EVIDENCE_STATUS=failed
  exit "$status"
fi

if [ "$RUN_MESH_RESOURCE" = "1" ]; then
  echo "OK: Mesh transport resource workload on $TESTBOX"
else
  echo "OK: xray_e2e + xray_mesh_transport_e2e + xray_vless_xhttp_e2e + shared_quota_xray_e2e on $TESTBOX"
fi
write_evidence_manifest passed
echo "evidence_log=$EVIDENCE_OUTPUT_PATH"
