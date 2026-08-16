#!/usr/bin/env bash
set -euo pipefail

# Exercise official host-managed fresh joins inside Docker init containers on
# codex-testbox. This runner never targets a VPS or local Docker.

TESTBOX="${TESTBOX:-codex-testbox}"
RUST_IMAGE="${RUST_IMAGE:-rust:1.96.0-bookworm}"
XP_TEST_IMAGE="${XP_TEST_IMAGE:-xp-fresh-join-candidate}"
REPO_ROOT="$(git rev-parse --show-toplevel)"
REPO_NAME="$(basename "$REPO_ROOT")"
PATH_HASH8="$(python3 -c 'import hashlib, os, sys; print(hashlib.sha256(os.path.realpath(sys.argv[1]).encode()).hexdigest()[:8])' "$REPO_ROOT")"
GIT_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
RUN_ID="$(date -u +%Y%m%d_%H%M%S)_$GIT_SHA"
WORKSPACE_SLUG="${REPO_NAME}__${PATH_HASH8}"
REMOTE_BASE="/srv/codex/workspaces/$USER"
REMOTE_WORKSPACE="$REMOTE_BASE/$WORKSPACE_SLUG"
REMOTE_RUN="$REMOTE_WORKSPACE/runs/$RUN_ID"
COMPOSE_PROJECT="$(python3 -c 'import re, sys; print(re.sub(r"[^a-z0-9_-]+", "_", sys.argv[1].lower()).strip("_")[:63])' "codex_${WORKSPACE_SLUG}_host_join_${RUN_ID}")"

echo "testbox=$TESTBOX"
echo "remote_run=$REMOTE_RUN"
echo "compose_project=$COMPOSE_PROJECT"

ssh -o BatchMode=yes "$TESTBOX" "mkdir -p '$REMOTE_RUN' '$REMOTE_WORKSPACE/cargo-home' '$REMOTE_WORKSPACE/rustup-home' '$REMOTE_WORKSPACE/host-join-musl-target' '$REMOTE_WORKSPACE/receipts'"
rsync -az --delete \
  --exclude '.git/' \
  --exclude 'target/' \
  --exclude 'node_modules/' \
  --exclude 'web/node_modules/' \
  --exclude 'web/dist/' \
  "$REPO_ROOT/" "$TESTBOX:$REMOTE_RUN/"

REMOTE_RUN_B64="$(printf '%s' "$REMOTE_RUN" | base64 | tr -d '\n')"
REMOTE_WORKSPACE_B64="$(printf '%s' "$REMOTE_WORKSPACE" | base64 | tr -d '\n')"
COMPOSE_PROJECT_B64="$(printf '%s' "$COMPOSE_PROJECT" | base64 | tr -d '\n')"
RUST_IMAGE_B64="$(printf '%s' "$RUST_IMAGE" | base64 | tr -d '\n')"
XP_TEST_IMAGE_B64="$(printf '%s' "$XP_TEST_IMAGE" | base64 | tr -d '\n')"

# Run a synced remote script rather than piping it through SSH. Docker Compose
# may read stdin, which must never be allowed to consume the control script.
ssh -o BatchMode=yes "$TESTBOX" \
  "REMOTE_RUN_B64='$REMOTE_RUN_B64' REMOTE_WORKSPACE_B64='$REMOTE_WORKSPACE_B64' COMPOSE_PROJECT_B64='$COMPOSE_PROJECT_B64' RUST_IMAGE_B64='$RUST_IMAGE_B64' XP_TEST_IMAGE_B64='$XP_TEST_IMAGE_B64' bash '$REMOTE_RUN/scripts/testbox/host-managed-fresh-join-remote.sh'"
