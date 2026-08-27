#!/usr/bin/env bash
set -euo pipefail

# Validate the host-managed nft admission transaction in an isolated privileged testbox.
# This is test infrastructure only; Docker/Compose remains outside the deployment contract.

TESTBOX="${TESTBOX:-codex-testbox}"
REPO_ROOT="$(git rev-parse --show-toplevel)"
REPO_NAME="$(basename "$REPO_ROOT")"
PATH_HASH8="$(printf '%s' "$(realpath "$REPO_ROOT")" | shasum -a 256 | cut -c1-8)"
RUN_ID="$(date -u +%Y%m%d_%H%M%S)_$$_$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
REMOTE_RUN="/srv/codex/workspaces/$USER/${REPO_NAME}__${PATH_HASH8}/runs/$RUN_ID"
CONTAINER="codex_${REPO_NAME}_${PATH_HASH8}_ingress_${RUN_ID}"

echo "testbox=$TESTBOX"
echo "remote_run=$REMOTE_RUN"
echo "container=$CONTAINER"

ssh -o BatchMode=yes "$TESTBOX" "mkdir -p '$REMOTE_RUN'"
cleanup() {
  set +e
  ssh -o BatchMode=yes "$TESTBOX" "docker rm -f '$CONTAINER' >/dev/null 2>&1 || true; rm -rf '$REMOTE_RUN'"
}
trap cleanup EXIT INT TERM

ssh -o BatchMode=yes "$TESTBOX" "cat > '$REMOTE_RUN/run.sh'" <<'REMOTE'
#!/usr/bin/env bash
set -euo pipefail

container="${1:?container name required}"
docker run --rm --name "$container" --cgroupns=host \
  --cap-drop=ALL --cap-add=NET_ADMIN --cap-add=NET_RAW alpine:3.22 \
  sh -euxc '
    apk add --no-cache nftables >/dev/null
    cgroup=$(sed -n "s/^0:://p" /proc/self/cgroup | sed "s#^/##")
    test -n "$cgroup"
    level=$(printf "%s" "$cgroup" | awk -F/ "{print NF}")
    cat >/tmp/xp-ingress-guard.nft <<EOF
table inet xp_ingress_guard {
  comment "xp-ops ingress-guard ownership v1"
  counter global_over_limit {}
  counter source_v4_over_limit {}
  counter source_v6_over_limit {}
  counter admitted_syns {}
  chain input {
    type filter hook input priority -300; policy accept;
    socket cgroupv2 level $level "$cgroup" iifname != "lo" tcp flags & (syn | ack | rst) == syn limit rate over 8/second burst 20 packets counter name global_over_limit drop
    socket cgroupv2 level $level "$cgroup" iifname != "lo" tcp flags & (syn | ack | rst) == syn meter source_v4 size 1024 { ip saddr timeout 60s limit rate over 3/second burst 8 packets } counter name source_v4_over_limit drop
    socket cgroupv2 level $level "$cgroup" iifname != "lo" tcp flags & (syn | ack | rst) == syn meter source_v6 size 1024 { ip6 saddr timeout 60s limit rate over 3/second burst 8 packets } counter name source_v6_over_limit drop
    socket cgroupv2 level $level "$cgroup" iifname != "lo" tcp flags & (syn | ack | rst) == syn counter name admitted_syns return
  }
}
EOF
    nft --check -f /tmp/xp-ingress-guard.nft
    nft -f /tmp/xp-ingress-guard.nft
    nft --json list table inet xp_ingress_guard | grep -q "xp-ops ingress-guard ownership v1"
    nft delete table inet xp_ingress_guard
  '
REMOTE
ssh -o BatchMode=yes "$TESTBOX" "chmod 0700 '$REMOTE_RUN/run.sh' && '$REMOTE_RUN/run.sh' '$CONTAINER'"
echo "host-managed ingress guard namespace smoke passed"
