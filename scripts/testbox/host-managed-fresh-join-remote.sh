#!/usr/bin/env bash
set -euo pipefail

REMOTE_RUN="$(printf '%s' "${REMOTE_RUN_B64:?}" | base64 -d)"
REMOTE_WORKSPACE="$(printf '%s' "${REMOTE_WORKSPACE_B64:?}" | base64 -d)"
COMPOSE_PROJECT="$(printf '%s' "${COMPOSE_PROJECT_B64:?}" | base64 -d)"
RUST_IMAGE="$(printf '%s' "${RUST_IMAGE_B64:?}" | base64 -d)"
XP_TEST_IMAGE="$(printf '%s' "${XP_TEST_IMAGE_B64:?}" | base64 -d)"
XP_HOST_IMAGE_PREFIX="$COMPOSE_PROJECT"
RECEIPT_PATH="$REMOTE_WORKSPACE/receipts/host-managed-fresh-join-${COMPOSE_PROJECT}.txt"
BUILDER_NAME="${COMPOSE_PROJECT}-musl-builder"
SOURCE_NAME="${COMPOSE_PROJECT}-xray-source"
COMPOSE_DIR="$REMOTE_RUN/scripts/testbox"
COMPOSE_FILE="compose-host-managed-fresh-join.yml"

cleanup() {
  local status=$?
  set +e
  if [ "$status" -ne 0 ]; then
    printf '%s\n' \
      "result=failed" \
      "project=$COMPOSE_PROJECT" \
      "exit_status=$status" > "$RECEIPT_PATH"
    if [ -d "$COMPOSE_DIR" ]; then
      (
        cd "$COMPOSE_DIR" || exit 0
        XP_TEST_IMAGE="$XP_TEST_IMAGE" XP_HOST_IMAGE_PREFIX="$XP_HOST_IMAGE_PREFIX" docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" logs --no-color
      ) >> "$RECEIPT_PATH" 2>&1 || true
    fi
  fi
  if [ -d "$COMPOSE_DIR" ]; then
    cd "$COMPOSE_DIR" || exit 0
    XP_TEST_IMAGE="$XP_TEST_IMAGE" XP_HOST_IMAGE_PREFIX="$XP_HOST_IMAGE_PREFIX" docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  docker rm -f "$BUILDER_NAME" "$SOURCE_NAME" >/dev/null 2>&1 || true
  docker image rm "$XP_HOST_IMAGE_PREFIX-leader" "$XP_HOST_IMAGE_PREFIX-systemd" "$XP_HOST_IMAGE_PREFIX-openrc" >/dev/null 2>&1 || true
  rm -rf "$REMOTE_RUN" >/dev/null 2>&1 || true
  return "$status"
}
trap cleanup EXIT INT TERM

cd "$COMPOSE_DIR"

docker image inspect "$XP_TEST_IMAGE" >/dev/null

mkdir -p artifacts web-dist-placeholder
printf '%s\n' '<!doctype html><title>host managed fresh join test</title>' > web-dist-placeholder/index.html

docker create --name "$SOURCE_NAME" "$XP_TEST_IMAGE" >/dev/null
docker cp "$SOURCE_NAME:/usr/local/bin/xray" artifacts/xray
docker rm "$SOURCE_NAME" >/dev/null

docker run --rm --name "$BUILDER_NAME" \
  --label "codex.scope=host-managed-fresh-join" \
  --label "codex.remote_run=$REMOTE_RUN" \
  --cap-drop=ALL \
  --cap-add=CHOWN --cap-add=DAC_OVERRIDE --cap-add=FOWNER --cap-add=SETGID --cap-add=SETUID \
  -e CARGO_HOME=/cargo-home -e RUSTUP_HOME=/rustup-home \
  -e CARGO_TARGET_DIR=/target \
  -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  -e CARGO_HTTP_MULTIPLEXING=false -e CARGO_NET_RETRY=8 \
  -v "$REMOTE_RUN:/workspace" \
  -v "$REMOTE_WORKSPACE/cargo-home:/cargo-home" \
  -v "$REMOTE_WORKSPACE/rustup-home:/rustup-home" \
  -v "$REMOTE_WORKSPACE/host-join-musl-target:/target" \
  -w /workspace "$RUST_IMAGE" bash -c '
    set -euo pipefail
    apt-get update
    apt-get install -y --no-install-recommends musl-tools pkg-config build-essential ca-certificates zip
    rustup target add x86_64-unknown-linux-musl
    mkdir -p web/dist
    cp scripts/testbox/web-dist-placeholder/index.html web/dist/index.html
    touch -d "2026-08-16 00:00:00 UTC" web/dist/index.html
    cargo build --release --locked --target x86_64-unknown-linux-musl --bin xp --bin xp-ops
    cp /target/x86_64-unknown-linux-musl/release/xp scripts/testbox/artifacts/xp
    cp /target/x86_64-unknown-linux-musl/release/xp-ops scripts/testbox/artifacts/xp-ops
    cd scripts/testbox/artifacts
    mkdir -p repos/XTLS/Xray-core/releases assets
    zip -q assets/Xray-linux-64.zip xray
    printf "%s\n" "{\"tag_name\":\"vtest\",\"prerelease\":false,\"published_at\":\"2026-08-16T00:00:00Z\",\"assets\":[{\"name\":\"Xray-linux-64.zip\",\"browser_download_url\":\"http://artifact:8080/assets/Xray-linux-64.zip\"}]}" > repos/XTLS/Xray-core/releases/latest
    chown -R "$HOST_UID:$HOST_GID" /workspace/scripts/testbox/artifacts /workspace/web/dist
  '

XP_TEST_IMAGE="$XP_TEST_IMAGE" XP_HOST_IMAGE_PREFIX="$XP_HOST_IMAGE_PREFIX" docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" build leader systemd openrc </dev/null
XP_TEST_IMAGE="$XP_TEST_IMAGE" XP_HOST_IMAGE_PREFIX="$XP_HOST_IMAGE_PREFIX" docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" up -d \
  certgen artifact leader leader-tls systemd systemd-tls openrc openrc-tls </dev/null

compose() {
  XP_TEST_IMAGE="$XP_TEST_IMAGE" XP_HOST_IMAGE_PREFIX="$XP_HOST_IMAGE_PREFIX" docker compose -p "$COMPOSE_PROJECT" -f "$COMPOSE_FILE" "$@"
}

wait_for() {
  local label="$1"
  shift
  local deadline=$((SECONDS + 90))
  until "$@" >/dev/null 2>&1; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "timed out waiting for $label" >&2
      compose logs --no-color >&2 || true
      return 1
    fi
    sleep 1
  done
}

wait_for "leader health" compose exec -T leader curl -fsS http://127.0.0.1:62416/api/health
wait_for "systemd manager" compose exec -T systemd systemctl is-system-running --wait
wait_for "openrc manager" compose exec -T openrc rc-status
wait_for "local Xray fixture" compose exec -T artifact wget -qO- http://127.0.0.1:8080/repos/XTLS/Xray-core/releases/latest

issue_join_token() {
  compose exec -T leader curl -fsS \
    -H 'Authorization: Bearer testbox-admin-token-0123456789abcdef' \
    -H 'Content-Type: application/json' \
    -d '{"ttl_seconds":600}' http://127.0.0.1:62416/api/admin/cluster/join-tokens \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["join_token"])'
}

deploy_node() {
  local service="$1"
  local token="$2"
  compose exec -T "$service" env \
    XP_OPS_GITHUB_API_BASE_URL=http://artifact:8080 \
    SSL_CERT_FILE=/tls/ca-bundle.pem \
    NO_PROXY=leader-tls,systemd,openrc,artifact \
    no_proxy=leader-tls,systemd,openrc,artifact \
    /usr/local/bin/xp-ops deploy \
      --xp-bin /opt/xp-candidate/xp --node-name "$service" --access-host "$service" \
      --no-cloudflare --no-ddns --api-base-url "https://$service" --join-token "$token" \
      --enable-services --non-interactive -y
}

assert_follower() {
  local service="$1"
  follower_is_ready() {
    compose exec -T "$1" curl -fsS "https://$1/api/cluster/info" \
      | python3 -c 'import json,sys; assert json.load(sys.stdin)["role"] == "follower"'
  }
  wait_for "$service follower" follower_is_ready "$service"
  compose exec -T "$service" test -f /var/lib/xp/data/cluster/metadata.json
  compose exec -T "$service" test -f /etc/xp/xp.env
  compose exec -T "$service" grep -q '^XP_ADMIN_TOKEN_HASH=' /etc/xp/xp.env
}

systemd_token="$(issue_join_token)"
deploy_node systemd "$systemd_token"
compose exec -T systemd systemctl is-active --quiet xray.service
compose exec -T systemd systemctl is-active --quiet xp.service
assert_follower systemd

openrc_token="$(issue_join_token)"
deploy_node openrc "$openrc_token"
compose exec -T openrc rc-service xray status
compose exec -T openrc rc-service xp status
assert_follower openrc

compose exec -T systemd systemctl restart xp.service
compose exec -T openrc rc-service xp restart
assert_follower systemd
assert_follower openrc

leader_nodes="$(compose exec -T leader curl -fsS -H 'Authorization: Bearer testbox-admin-token-0123456789abcdef' http://127.0.0.1:62416/api/admin/nodes)"
printf '%s' "$leader_nodes" | python3 -c 'import json,sys; names={item["node_name"] for item in json.load(sys.stdin)["items"]}; assert {"leader","systemd","openrc"} <= names, names'

printf '%s\n' \
  "result=passed" \
  "project=$COMPOSE_PROJECT" \
  "deploy=official-xp-ops" \
  "nodes=leader,systemd,openrc" \
  "roles=systemd:follower,openrc:follower" \
  "restart_identity=preserved" > "$RECEIPT_PATH"

echo "host-managed fresh join passed: systemd and OpenRC deployed through xp-ops"
