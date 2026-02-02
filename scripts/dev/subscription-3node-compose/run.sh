#!/usr/bin/env sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

compose() {
  if docker compose version >/dev/null 2>&1; then
    docker compose -f "$SCRIPT_DIR/docker-compose.yml" "$@"
  else
    docker-compose -f "$SCRIPT_DIR/docker-compose.yml" "$@"
  fi
}

need_python3() {
  if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required" >&2
    exit 1
  fi
}

ensure_ports() {
  if [ -n "${XP_APXDG_XP1_HTTPS_PORT:-}" ] || [ -n "${XP_APXDG_XP2_HTTPS_PORT:-}" ] || [ -n "${XP_APXDG_XP3_HTTPS_PORT:-}" ]; then
    if [ -z "${XP_APXDG_XP1_HTTPS_PORT:-}" ] || [ -z "${XP_APXDG_XP2_HTTPS_PORT:-}" ] || [ -z "${XP_APXDG_XP3_HTTPS_PORT:-}" ]; then
      echo "either set all XP_APXDG_XP{1,2,3}_HTTPS_PORT or set none (let Docker pick ephemeral ports)" >&2
      exit 1
    fi
    export XP_APXDG_XP1_HTTPS_PORT XP_APXDG_XP2_HTTPS_PORT XP_APXDG_XP3_HTTPS_PORT
  fi
}

ensure_admin_token() {
  if [ -z "${XP_APXDG_ADMIN_TOKEN:-}" ]; then
    XP_APXDG_ADMIN_TOKEN="devtoken"
  fi
  if [ -z "${XP_APXDG_ADMIN_TOKEN_HASH:-}" ]; then
    XP_APXDG_ADMIN_TOKEN_HASH='$argon2id$v=19$m=65536,t=3,p=1$6uKH2kKC9AT6hehxo9FSkA$2CLHgqDffZmHkDhywvDK59us3WlSXc0rX1rE1zbKi/U'
  fi
  export XP_APXDG_ADMIN_TOKEN XP_APXDG_ADMIN_TOKEN_HASH
}

wait_https_ok() {
  name="$1"
  path="$2"
  i=0
  echo "waiting for https://${name}:6443${path} ..."
  while :; do
    if compose exec -T tool curl -fsS --connect-timeout 1 --max-time 2 \
      --cacert /vol/xp1/cluster/cluster_ca.pem \
      "https://${name}:6443${path}" >/dev/null 2>&1; then
      break
    fi
    i=$((i + 1))
    if [ "$i" -gt 600 ]; then
      echo "timeout waiting for ${name}" >&2
      compose logs --no-color "$name" || true
      compose logs --no-color "${name}-app" || true
      exit 1
    fi
    sleep 0.2
  done
}

json_get() {
  need_python3
  key="$1"
  python3 -c '
import json
import sys

key = sys.argv[1]
try:
    obj = json.load(sys.stdin)
except Exception as e:
    print(f"json_get: invalid json input: {e}", file=sys.stderr)
    sys.exit(1)

if key not in obj:
    print(f"json_get: missing key: {key}", file=sys.stderr)
    sys.exit(1)

print(obj[key])
' "$key"
}

reset() {
  ensure_ports
  ensure_admin_token
  echo "WARNING: reset will wipe docker volumes for compose project \"xp-apxdg\" (data loss)" >&2
  compose down -v --remove-orphans
}

build() {
  ensure_ports
  ensure_admin_token
  if [ "${XP_APXDG_FORCE_BUILD:-}" = "1" ]; then
    DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 compose build xp1-app
    return 0
  fi

  if docker image inspect xp-local:apxdg >/dev/null 2>&1; then
    echo "image xp-local:apxdg already exists; skipping build (set XP_APXDG_FORCE_BUILD=1 to rebuild)"
    return 0
  fi

  DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 compose build xp1-app
}

init_leader_and_cert() {
  ensure_ports
  ensure_admin_token

  compose run --rm --no-deps xp1-app init --data-dir /data --node-name node-1 --access-host xp1 --api-base-url https://xp1:6443

  compose run --rm --no-deps --entrypoint sh xp1-app -c '
set -eu
cd /data/cluster
if [ ! -f cluster_ca.pem ] || [ ! -f cluster_ca_key.pem ]; then
  echo "missing cluster ca files under /data/cluster" >&2
  exit 1
fi

rm -f https_key.pem https_cert.pem https_cert.srl https_cert.csr openssl.cnf

cat > openssl.cnf <<EOF
[req]
distinguished_name=req_dn
prompt=no
req_extensions=req_ext

[req_dn]
CN=xp-apxdg

[req_ext]
subjectAltName=@alt_names
extendedKeyUsage=serverAuth

[alt_names]
DNS.1=xp1
DNS.2=xp2
DNS.3=xp3
DNS.4=localhost
IP.1=127.0.0.1
EOF

openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out https_key.pem
openssl req -new -key https_key.pem -out https_cert.csr -config openssl.cnf
openssl x509 -req -in https_cert.csr -CA cluster_ca.pem -CAkey cluster_ca_key.pem -CAcreateserial -out https_cert.pem -days 3650 -sha256 -extensions req_ext -extfile openssl.cnf
chmod 0600 https_key.pem
'
}

copy_https_cert_to_joiner() {
  from="$1"
  to="$2"
  compose exec -T tool sh -c "
set -eu
mkdir -p /vol/${to}/cluster
cp /vol/${from}/cluster/https_cert.pem /vol/${to}/cluster/https_cert.pem
cp /vol/${from}/cluster/https_key.pem /vol/${to}/cluster/https_key.pem
"
}

create_join_token() {
  ensure_admin_token
  out="$(
    compose exec -T xp1-app curl -fsSL \
      --cacert /data/cluster/cluster_ca.pem \
      -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
      -H "Content-Type: application/json" \
      -d '{"ttl_seconds":300}' \
      https://xp1:6443/api/admin/cluster/join-tokens
  )"
  printf '%s\n' "$out"
}

create_join_token_value() {
  ensure_admin_token
  need_python3

  i=0
  while :; do
    out="$(create_join_token 2>/dev/null || true)"
    if [ -n "$out" ]; then
      if token="$(printf '%s' "$out" | json_get join_token 2>/dev/null)"; then
        printf '%s\n' "$token"
        return 0
      fi
    fi

    i=$((i + 1))
    if [ "$i" -gt 200 ]; then
      echo "timeout creating join token from leader" >&2
      if [ -n "$out" ]; then
        echo "last response: $out" >&2
      fi
      compose logs --no-color xp1 || true
      compose logs --no-color xp1-app || true
      exit 1
    fi
    sleep 0.2
  done
}

join_node() {
  node="$1"
  token="$2"

  node_name="node-${node#xp}"
  compose run --rm --no-deps "${node}-app" join \
    --token "$token" \
    --data-dir /data \
    --node-name "$node_name" \
    --access-host "$node" \
    --api-base-url "https://${node}:6443"
}

up() {
  ensure_ports
  ensure_admin_token

  build
  compose up -d tool
  init_leader_and_cert

  compose up -d xp1-app xp1
  wait_https_ok xp1 "/api/cluster/info"

  t2="$(create_join_token_value)"
  join_node xp2 "$t2"
  copy_https_cert_to_joiner xp1 xp2
  compose up -d xp2-app xp2
  wait_https_ok xp2 "/api/cluster/info"

  t3="$(create_join_token_value)"
  join_node xp3 "$t3"
  copy_https_cert_to_joiner xp1 xp3
  compose up -d xp3-app xp3
  wait_https_ok xp3 "/api/cluster/info"

  echo "cluster is up"
  urls
}

seed() {
  ensure_admin_token
  need_python3

  users_list="$(
    compose exec -T xp1-app curl -fsS \
      --cacert /data/cluster/cluster_ca.pem \
      -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
      https://xp1:6443/api/admin/users
  )"

  user_id="$(
    printf '%s' "$users_list" | python3 -c '
import json
import sys

obj = json.load(sys.stdin)
for u in obj.get("items", []):
    if u.get("display_name") == "alice":
        print(u["user_id"])
        raise SystemExit(0)
print("")
'
  )"

  if [ -z "$user_id" ]; then
    users_json="$(
      compose exec -T xp1-app curl -fsS \
        --cacert /data/cluster/cluster_ca.pem \
        -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
        -H "Content-Type: application/json" \
        -d '{"display_name":"alice"}' \
        https://xp1:6443/api/admin/users
    )"

    user_id="$(echo "$users_json" | json_get user_id)"
  fi

  nodes_json="$(
    compose exec -T xp1-app curl -fsS \
      --cacert /data/cluster/cluster_ca.pem \
      -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
      https://xp1:6443/api/admin/nodes
  )"

  node1_id="$(
    printf '%s' "$nodes_json" | python3 -c '
import json
import sys

obj = json.load(sys.stdin)
items = obj["items"]

def pick(prefix: str) -> str:
    for n in items:
        if n["api_base_url"].startswith(prefix):
            return n["node_id"]
    raise SystemExit("node not found for prefix: %s" % prefix)

print(pick("https://xp1:"))
'
  )"

  node2_id="$(
    printf '%s' "$nodes_json" | python3 -c '
import json
import sys

obj = json.load(sys.stdin)
items = obj["items"]

def pick(prefix: str) -> str:
    for n in items:
        if n["api_base_url"].startswith(prefix):
            return n["node_id"]
    raise SystemExit("node not found for prefix: %s" % prefix)

print(pick("https://xp2:"))
'
	  )"

  endpoints_list="$(
    compose exec -T xp1-app curl -fsS \
      --cacert /data/cluster/cluster_ca.pem \
      -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
      https://xp1:6443/api/admin/endpoints
  )"

  find_endpoint_id() {
    node_id="$1"
    port="$2"
    kind="$3"
    printf '%s' "$endpoints_list" | python3 -c '
import json
import sys

node_id = sys.argv[1]
port = int(sys.argv[2])
kind = sys.argv[3]

obj = json.load(sys.stdin)
for e in obj.get("items", []):
    if e.get("node_id") == node_id and e.get("kind") == kind and e.get("port") == port:
        print(e["endpoint_id"])
        raise SystemExit(0)
print("")
' "$node_id" "$port" "$kind"
  }

  create_endpoint() {
    node_id="$1"
    port="$2"
    compose exec -T xp1-app curl -fsS \
      --cacert /data/cluster/cluster_ca.pem \
      -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
      -H "Content-Type: application/json" \
      -d "{\"node_id\":\"${node_id}\",\"kind\":\"ss2022_2022_blake3_aes_128_gcm\",\"port\":${port}}" \
      https://xp1:6443/api/admin/endpoints
  }

  kind="ss2022_2022_blake3_aes_128_gcm"
  e1="$(find_endpoint_id "$node1_id" 31081 "$kind")"
  if [ -z "$e1" ]; then e1="$(create_endpoint "$node1_id" 31081 | json_get endpoint_id)"; fi
  e2="$(find_endpoint_id "$node1_id" 31082 "$kind")"
  if [ -z "$e2" ]; then e2="$(create_endpoint "$node1_id" 31082 | json_get endpoint_id)"; fi
  e3="$(find_endpoint_id "$node2_id" 31083 "$kind")"
  if [ -z "$e3" ]; then e3="$(create_endpoint "$node2_id" 31083 | json_get endpoint_id)"; fi
  e4="$(find_endpoint_id "$node2_id" 31084 "$kind")"
  if [ -z "$e4" ]; then e4="$(create_endpoint "$node2_id" 31084 | json_get endpoint_id)"; fi

  group_name="apxdg"
  if compose exec -T xp1-app curl -fsS \
    --cacert /data/cluster/cluster_ca.pem \
    -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
    "https://xp1:6443/api/admin/grant-groups/${group_name}" >/dev/null 2>&1; then
    compose exec -T xp1-app curl -fsS \
      --cacert /data/cluster/cluster_ca.pem \
      -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
      -H "Content-Type: application/json" \
      -X PUT \
      -d "{
        \"members\":[
          {\"user_id\":\"${user_id}\",\"endpoint_id\":\"${e1}\",\"enabled\":true,\"quota_limit_bytes\":0,\"note\":\"same\"},
          {\"user_id\":\"${user_id}\",\"endpoint_id\":\"${e2}\",\"enabled\":true,\"quota_limit_bytes\":0,\"note\":\"same\"},
          {\"user_id\":\"${user_id}\",\"endpoint_id\":\"${e3}\",\"enabled\":true,\"quota_limit_bytes\":0,\"note\":\"same\"},
          {\"user_id\":\"${user_id}\",\"endpoint_id\":\"${e4}\",\"enabled\":true,\"quota_limit_bytes\":0,\"note\":\"same\"}
        ]
      }" \
      "https://xp1:6443/api/admin/grant-groups/${group_name}" >/dev/null
  else
    compose exec -T xp1-app curl -fsS \
      --cacert /data/cluster/cluster_ca.pem \
      -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
      -H "Content-Type: application/json" \
      -d "{
        \"group_name\":\"${group_name}\",
        \"members\":[
          {\"user_id\":\"${user_id}\",\"endpoint_id\":\"${e1}\",\"enabled\":true,\"quota_limit_bytes\":0,\"note\":\"same\"},
          {\"user_id\":\"${user_id}\",\"endpoint_id\":\"${e2}\",\"enabled\":true,\"quota_limit_bytes\":0,\"note\":\"same\"},
          {\"user_id\":\"${user_id}\",\"endpoint_id\":\"${e3}\",\"enabled\":true,\"quota_limit_bytes\":0,\"note\":\"same\"},
          {\"user_id\":\"${user_id}\",\"endpoint_id\":\"${e4}\",\"enabled\":true,\"quota_limit_bytes\":0,\"note\":\"same\"}
        ]
      }" \
      https://xp1:6443/api/admin/grant-groups >/dev/null
  fi

  echo "seed ok (user=alice, 4 endpoints, 4 grants in group=\"${group_name}\" with note=\"same\")"
}

verify_one_raw() {
  node="$1"
  token="$2"
  compose exec -T xp1-app curl -fsS --cacert /data/cluster/cluster_ca.pem \
    "https://${node}:6443/api/sub/${token}?format=raw"
}

verify_one_clash() {
  node="$1"
  token="$2"
  compose exec -T xp1-app curl -fsS --cacert /data/cluster/cluster_ca.pem \
    "https://${node}:6443/api/sub/${token}?format=clash"
}

verify() {
  ensure_admin_token
  need_python3

  users_json="$(
    compose exec -T xp1-app curl -fsS \
      --cacert /data/cluster/cluster_ca.pem \
      -H "Authorization: Bearer ${XP_APXDG_ADMIN_TOKEN}" \
      https://xp1:6443/api/admin/users
  )"

  sub_token="$(
    printf '%s' "$users_json" | python3 -c '
import json
import sys

obj = json.load(sys.stdin)
for u in obj["items"]:
    if u.get("display_name") == "alice":
        print(u["subscription_token"])
        raise SystemExit(0)
raise SystemExit("alice not found; did you run seed?")
'
  )"

  raw1="$(verify_one_raw xp1 "$sub_token")"
  raw2="$(verify_one_raw xp2 "$sub_token")"
  raw3="$(verify_one_raw xp3 "$sub_token")"

  printf '%s\n' "$raw1" | python3 -c '
import sys

raw = sys.stdin.read().splitlines()
lines = [l for l in raw if l.strip()]
assert len(lines) == 4, f"raw lines expected 4, got {len(lines)}"

names = []
for l in lines:
    if "#" not in l:
        raise AssertionError("raw line missing #name: " + l)
    names.append(l.rsplit("#", 1)[1])

assert len(set(names)) == 4, f"raw names not unique: {names}"
print("raw ok (4 lines, unique names)")
'

  if [ "$raw1" != "$raw2" ] || [ "$raw1" != "$raw3" ]; then
    echo "raw output mismatch between nodes" >&2
    exit 1
  fi

  clash1="$(verify_one_clash xp1 "$sub_token")"
  clash2="$(verify_one_clash xp2 "$sub_token")"
  clash3="$(verify_one_clash xp3 "$sub_token")"

  printf '%s\n' "$clash1" | python3 -c '
import re
import sys

text = sys.stdin.read().splitlines()
names = []
for line in text:
    m = re.match(r"^\s*-\s*name:\s*(.*)\s*$", line)
    if not m:
        continue
    v = m.group(1).strip()
    if v.startswith("\"") and v.endswith("\"") and len(v) >= 2:
        v = v[1:-1]
    if v.startswith(chr(39)) and v.endswith(chr(39)) and len(v) >= 2:
        v = v[1:-1]
    names.append(v)
assert len(names) == 4, f"clash proxies expected 4, got {len(names)} ({names})"
assert len(set(names)) == 4, f"clash proxy names not unique: {names}"
print("clash ok (4 proxies, unique names)")
'

  if [ "$clash1" != "$clash2" ] || [ "$clash1" != "$clash3" ]; then
    echo "clash output mismatch between nodes" >&2
    exit 1
  fi

  echo "verify ok (xp1/xp2/xp3 consistent)"
}

logs() {
  ensure_ports
  ensure_admin_token
  compose logs --no-color
}

urls() {
  ensure_ports
  ensure_admin_token
  xp1_addr="$(compose port xp1 6443 2>/dev/null || true)"
  xp2_addr="$(compose port xp2 6443 2>/dev/null || true)"
  xp3_addr="$(compose port xp3 6443 2>/dev/null || true)"
  if [ -n "$xp1_addr" ]; then echo "xp1: https://${xp1_addr}"; else echo "xp1: <not running>"; fi
  if [ -n "$xp2_addr" ]; then echo "xp2: https://${xp2_addr}"; else echo "xp2: <not running>"; fi
  if [ -n "$xp3_addr" ]; then echo "xp3: https://${xp3_addr}"; else echo "xp3: <not running>"; fi
  echo "admin token: ${XP_APXDG_ADMIN_TOKEN}"
}

reset_and_verify() {
  reset
  up
  seed
  verify
}

cmd="${1:-}"
case "$cmd" in
  reset) reset ;;
  build) build ;;
  up) up ;;
  seed) seed ;;
  verify) verify ;;
  logs) logs ;;
  urls) urls ;;
  reset-and-verify) reset_and_verify ;;
  ""|-h|--help|help)
    echo "usage: $0 {reset|build|up|seed|verify|reset-and-verify|urls|logs}" >&2
    exit 2
    ;;
  *)
    echo "unknown command: $cmd" >&2
    exit 2
    ;;
esac
