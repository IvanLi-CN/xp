# Ops: host-managed and container deployments

This directory contains both the traditional host-managed service examples and the single-image Docker deployment guide.

- Host-managed services (systemd/OpenRC): this document
- Single-image Docker runtime: `docs/ops/docker.md`
- Owner-facing Docker deployment walkthrough: `docs/ops/docker-deployment-guide.md`

## Supported deployment matrix

`xp` is expected to remain deployable across these owner-facing environments:

| Deployment shape            | Runtime manager              | Status          | Typical node class                                                    | Notes                                                                                                 |
| --------------------------- | ---------------------------- | --------------- | --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Host-managed service node   | systemd                      | fully supported | host-managed service node with init-managed `xp + xray + cloudflared` | `xp`, `xray`, and optional `cloudflared` are installed on the host and managed by systemd             |
| Host-managed service node   | OpenRC                       | fully supported | host-managed service node with init-managed `xp + xray + cloudflared` | `xp`, `xray`, and optional `cloudflared` are installed on the host and managed by OpenRC              |
| Single-image container node | Docker Compose / OCI runtime | fully supported | official single-image container node                                  | `xp-ops container run` owns bootstrap/join, child process supervision, and missing endpoint bootstrap |

Current support boundaries that operators must know:

- Host-managed automation in `xp-ops` currently recognizes Arch/Debian/Ubuntu/RHEL-family/Alpine distro families. Historical CentOS 7 / RHEL-family host-managed nodes are first-class host-managed targets and should use the host-managed deployment / upgrade paths in this document.
- Feature delivery must not be container-only. Runtime contracts such as managed-default endpoint reconcile, VLESS HTTPS canary fallback, Mihomo relay URL generation, and upgrade-time auto-adoption must behave the same way once a node is running, regardless of whether the node is host-managed or container-managed.
- Managed-default endpoint ports are cluster-owned after creation or auto-adoption.
  `XP_DEFAULT_VLESS_PORT` and `XP_DEFAULT_SS_PORT` only bootstrap a missing endpoint; changing or
  removing those env values does not reconfigure or delete an existing endpoint.
- When a deployment environment needs manual intervention, document the exact branch and operator steps instead of implying the generic path will work.

## Web PWA and API compatibility

The embedded admin Web app is a build-versioned PWA. A release precaches the complete HTML,
JavaScript, CSS, font, icon, and manifest app shell under a build-specific cache name.
The new worker waits for the operator's confirmation before activation. An interrupted install leaves
the active build usable. Do not manually delete `xp-app-shell-*` caches while an older tab may still be
open. Cross-tab ownership is stored in `xp_sw_metadata`, separately from the React Query cache.

The only activation exception repairs an old Workbox controller that cannot render the XP update
prompt. After a complete new app shell is verified, the Worker may wait up to one second for a
declaration from live clients. It activates in the background only when the exact same-scope
`workbox-precache-v2-<scope>` exists and no client has valid XP ownership.
This migration never claims or refreshes an open page; the operator must refresh or reopen it.
`xp_sw_metadata` records the precise legacy cache and pre-existing orphan XP app-shell names.
Those caches are deleted only after every live client declares a valid XP build. Normal XP-to-XP
updates still wait for the operator's confirmation.

The Web client supports the current API minor and the two previous minors in the fixed `3.22`,
`3.21`, and `3.20` window. It probes `GET /api/capabilities`, falls back to the strict current
release tag from `GET /api/version/check`, and finally uses local endpoint fingerprints. A missing
capability disables only the affected UI feature; an endpoint or schema failure for a declared capability
is an API regression and must be investigated as such. PWA build IDs and API release profiles are
independent and must not be manually aligned.

## Minimal runtime assumptions

Host-managed mode assumptions:

- `xp` runs as a local HTTP admin/API server and binds loopback by default (`127.0.0.1:62416`).
- `xray` runs locally and exposes its gRPC API on loopback by default (`127.0.0.1:10085`).
- `xp` talks to `xray` via gRPC at `XP_XRAY_API_ADDR`.
- `xp` uses managed VLESS/REALITY and the peer `api_base_url` Tunnel/public origin as equal peer-direct control-plane paths. Repository synchronization may use a separate in-memory dynamic relay only after both direct paths fail.
- A configured history repository persists its replica state in `${XP_DATA_DIR}/history.sqlite3`.
  Membership, lifecycle and capacity are Raft-backed; `GET /api/admin/history-repositories`
  reports configured, partial and unreachable states with per-member capacity and sync quality.
  `PUT /api/admin/history-repositories` replaces the validated membership through Raft. There is
  no static Mesh proxy environment, listener or compatibility path.
- Nodes exposes the same repository status and a membership editor. The editor selects existing
  cluster nodes; `PUT /api/admin/history-repositories` accepts only `node_ids` and derives pinned
  repository identities server-side. Lifecycle, convergence and capacity remain worker-owned. A
  new member remains `syncing` while it reads bounded repair batches from existing `ready` members;
  only a successful catch-up followed by five stable minutes transitions it to `ready`.
  Repository query views accept a bounded `subject_node_id` filter and display observed/received
  coverage, watermarks, gaps, skew, completeness, and a next-page control.
- During `xp-ops init` or upgrade, XP may remove an already-stored legacy `mesh-proxy` Xray
  inbound and its routing rules. This is removal-only migration cleanup: it has no configuration
  input, client, metric, route, fallback or compatibility behavior, and the removed artifacts are
  not recreated.
- `xp` periodically probes `xray` and exposes status via `GET /api/health` (`xray.*` fields). On `down -> up`, `xp` requests a full reconcile.
- `xray` is supervised by the init system (systemd/OpenRC). `xp` does not spawn `xray`, but it can request a restart through the init system (requires a minimal permission policy installed by `xp-ops`).
- `xp` also tracks `cloudflared` when `XP_CLOUDFLARED_MONITOR_MODE!=none`. `XP_CLOUDFLARED_RESTART_MODE` separately controls whether `xp` may actively request a Tunnel restart; host-managed OpenRC defaults should monitor cloudflared but leave active restarts disabled.
- `xp` records runtime status transitions/restart outcomes to `${XP_DATA_DIR}/service_runtime.json` for the Web runtime pages.
- When `XP_CLOUDFLARE_DDNS_ENABLED=true`, `xp` also reconciles `XP_ACCESS_HOST` against Cloudflare DNS (`A` / `AAAA`) and stores local DDNS state in `${XP_DATA_DIR}/ddns_state.json`.
- `xp` can also run a loopback-only VLESS HTTPS canary (`XP_VLESS_CANARY_BIND`, default `127.0.0.1:39043`). xp-managed VLESS/REALITY endpoints send unauthenticated HTTPS fallback traffic to this canary and expose its runtime / certificate state through `GET /api/health` and `GET /api/admin/config`.

## Low-memory host defaults

Host-managed deployments are expected to run on small VPS/LXC machines, including `256MB` RAM without swap. The default recovery contract is:

- `XP_XRAY_HEALTH_INTERVAL_SECS=5`
- `XP_XRAY_HEALTH_FAILS_BEFORE_DOWN=4`
- `XP_XRAY_RESTART_COOLDOWN_SECS=30`
- `XP_XRAY_RESTART_TIMEOUT_SECS=20`
- `XP_CLOUDFLARED_MONITOR_MODE=<init-system>`
- `XP_CLOUDFLARED_RESTART_MODE=none`

This keeps the first xray restart within roughly `30-60s` from an actual failure while avoiding repeated restarts when the host is under memory or I/O pressure. If a component remains down after restart attempts, `xp` increases the next restart delay exponentially up to 300 seconds and resets that delay after the probe recovers.

## Endpoint probe (ingress reachability)

`xp` runs a cluster-wide probe to measure **reachability** and **latency** for every endpoint (last 24 hours, per-hour buckets).

For probe semantics and troubleshooting notes (including what is and is not allowed to "work around"), see:

- `docs/ops/endpoint-probe.md`

## Optional: public access via Cloudflare Tunnel

If you want to reach `xp` from the public Internet without opening inbound ports, see:

- `docs/ops/cloudflare-tunnel.md`

Notes:

- `xp-ops deploy` supports passing the Cloudflare API token via `--cloudflare-token` (riskier) or `--cloudflare-token-stdin` (preferred over the flag).
- Token resolution priority for deploy is: `flag/stdin` → `CLOUDFLARE_API_TOKEN` → `/etc/xp-ops/cloudflare_tunnel/api_token`.
- `xp-ops deploy --ddns` reuses that token source, then writes an `xp`-readable runtime copy to `/etc/xp/cloudflare_ddns_api_token`.
- For a host-managed join, deploy provisions Tunnel/DNS without starting `cloudflared`, runs
  `xp join`, writes `/etc/xp/xp.env`, then enables, starts or restarts, and confirms `xray`, `xp`, and optional
  `cloudflared` in that order. With `--enable-services`, final `https://<api-base-url>/health`
  must return HTTP `200`. A `post_join_health_failed` result retains the joined member and its
  metadata; rerun deploy after repairing the service or public routing without issuing a new join
  token.

## Optional: managed VLESS HTTPS canary

If you want Mihomo relay `url-test` to probe the actual managed VLESS ingress instead of the admin API origin, configure the loopback TLS canary:

- `XP_VLESS_CANARY_BIND=127.0.0.1:39043` by default.
- `XP_VLESS_CANARY_ACME_DIRECTORY_URL` defaults to Let's Encrypt production.
- `XP_VLESS_CANARY_ACME_CONTACT_EMAIL` is optional but recommended.
- `XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE` defaults to `/etc/xp/cloudflare_ddns_api_token` so host-managed nodes can reuse the same xp-readable Cloudflare runtime token as DDNS.
- `XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID` is optional; when empty, `xp` first reuses `XP_CLOUDFLARE_DDNS_ZONE_ID` when present, and only falls back to deriving the Cloudflare zone from `XP_ACCESS_HOST` when the DDNS zone is also unset.
- `XP_VLESS_CANARY_DNS_PROPAGATION_TIMEOUT_SECS` defaults to `180`; `xp` waits until the DNS-01 TXT is visible through both Cloudflare and Google DNS-over-HTTPS resolvers before asking the ACME server to validate it. The propagation check does not require direct UDP/TCP 53 access to authoritative nameservers.

Contract:

- `xp` terminates TLS for `GET/HEAD /generate_204` on the loopback canary and returns `204`.
- xp-managed/default VLESS/REALITY endpoints set `reality.dest` to that loopback canary, and set `server_names` to `[XP_ACCESS_HOST]` without a port. `XP_DEFAULT_VLESS_SERVER_NAMES` is deprecated compatibility input; it is validated only when `XP_DEFAULT_VLESS_PORT` activates bootstrap, and it does not choose managed VLESS SNI.
- The canary routes non-probe HTTPS traffic by HTTP authority, not by TLS SNI. `Host` / HTTP/2 `:authority` always accepts the canonical `XP_ACCESS_HOST[:endpoint_port]`; `:443` may be omitted. Managed VLESS endpoints may also carry an extra `accepted_authorities` set of normalized `host[:port]` aliases; omitting the port means HTTPS default `443`. Exactly one managed VLESS endpoint on the node must match the canonical authority or one of those aliases.
- Each managed VLESS endpoint may store its own `canary_upstream` origin URL and `accepted_authorities` alias set. `accepted_authorities` only affects ordinary HTTPS Host matching; it does not change REALITY `server_names`, `reality.dest`, or the canonical `/generate_204` probe URL. When `canary_upstream` is unset, non-probe requests now return a plain text `404 Not Found`. When set, xp forwards method, path, query, non-hop-by-hop headers, status, response headers, and streaming bodies to that endpoint upstream. The outbound `Host` is normalized to the `canary_upstream` origin so localhost and name-based upstream services work predictably. Upstream mode is `auto`, `http1`, or explicit `h2c`; `auto` supports HTTP/1.1 and HTTPS ALPN HTTP/2.
- Admin UI 的默认 `New endpoint` VLESS 创建路径已收敛到同一托管合同：页面只提交 `port` 与可选 `canary_upstream` / `accepted_authorities`，服务端按节点 `access_host` 自动派生 `reality.dest=XP_VLESS_CANARY_BIND`、`server_names=[node.access_host]` 并写入 `managed_default=true`。legacy 非托管 VLESS 创建仅保留给显式 API 客户端兼容，不再是 UI 主路径。
- The reverse proxy is TLS-terminating HTTP reverse proxy behavior, not TCP passthrough and not a forward proxy. It supports streaming request/response bodies, SSE, large uploads/downloads, and WebSocket upgrade over an HTTP/1.1 upstream connection; explicit `h2c` is for non-upgrade HTTP traffic, and `CONNECT` is not part of the v1 contract.
- Ordinary HTTPS clients probing `https://<access_host[:vless_port]>/generate_204` receive the canary `204` through the VLESS ingress itself and never touch upstream.
- The endpoint detail page exposes a managed VLESS **Canary /generate_204** test for that ordinary HTTPS path. It fans out to every xp node, reports per-node status/latency/error, and is an immediate diagnostic for public ingress, TLS, REALITY fallback, and xp canary behavior; it is separate from the hourly cluster-wide proxy path probe and is not stored in endpoint probe history.
- Host-managed and container-managed nodes use the same managed-default endpoint contract. On host-managed nodes, `xp` startup and `xp-ops xp sync-node-meta` both reconcile the local default endpoint set; on container-managed nodes, `xp-ops container run` does the same after the local control plane is ready.
- Existing managed-default VLESS and SS2022 ports come from Raft state. Reconcile refreshes
  system-managed metadata but preserves the endpoint port when the local bootstrap env differs or
  is absent.
- Historical host-managed nodes with exactly one legacy VLESS endpoint on the node are auto-adopted
  into the managed-default contract during upgrade when that endpoint still predates the
  `managed_default` metadata flag; auto-adoption preserves the legacy endpoint port. The runtime
  only rewrites that ingress to the loopback canary semantics after the canary itself is ready. If
  canary preparation fails, the old ingress stays untouched while
  `vless_https_canary_status.last_error` explains the blocker.
- This does not move the admin UI / cluster API onto the VLESS port.
- Mihomo relay groups prefer `https://<access_host[:managed_vless_port]>/generate_204`, then fall back to `api_base_url + /api/health`, then `https://www.gstatic.com/generate_204`.
- Legacy `XP_RELAY_PROBE_*` variables are removed; startup/sync now fails fast if they are still present.

## Reality fallback control-plane Mesh

When a peer has exactly one managed-default VLESS/REALITY endpoint, XP derives
`https://<access_host>:<vless_port>` as a signed control-plane Mesh route. The canary keeps
ordinary `/generate_204` and authority-based camouflage traffic separate from Mesh traffic:

- signed `health-v2` requests reach only the bodyless Mesh health endpoint;
- signed `mesh-v2` requests may reach only `/raft/*` and `/api/admin/_internal/*` on the fixed
  local XP loopback origin;
- malformed or unsigned requests carrying reserved `X-XP-*` route headers return `404` and are
  never forwarded to a camouflage upstream.

Each peer has an independent breaker. After three retryable Mesh transport failures it uses the
`30/60/120/240/300s` backoff sequence and sends the remaining request budget to the public
`api_base_url`. A signed acknowledgement is authoritative even for a non-2xx HTTP result, so it
never triggers a second path attempt. The local node keeps 24 hours of one-minute buckets and the
last 200 global transitions in `${XP_DATA_DIR}/mesh/telemetry.json`; inspect them with
`GET /api/admin/mesh/status` or the Web **System status** page. `POST /api/admin/mesh/probes`
accepts only current remote member IDs.

Raft, leader forwarding, node history, probes, runtime, alerts, quota, traffic, IP usage, TCP
history, endpoint probes and SSE share one process-wide Mesh client. Its managed route is
HTTP/2-only, retains at most one idle connection per origin for 120 seconds, and does not send H2
PING frames. The normal 60-second probe cadence keeps an active peer connection reusable. Public
direct and optional relay fallback use separate long-lived compatibility clients, so the strict H2
contract never changes public-origin compatibility. H2 transport failures enter the existing
breaker/fallback path; invalid authentication or acknowledgement remains terminal and never
downgrades to public transport.

When the `admin.mesh-transport-reuse` capability is present, each peer status may report protocol,
connection generation, current-generation request count, and 5-minute/1-hour request and connection
start totals. These values contain no host, IP, socket, port or certificate identity. A healthy peer
uses H2 and starts at most two connections in five minutes; more starts or a protocol mismatch is
`churning`, while a peer without evidence is `unknown`. Public fallback retains the last Mesh reuse
evidence for diagnosis.

The node TCP chart remains an aggregate of business ingress connections and Mesh, and its 24-hour
peak remains visible until the historical window rolls over. After an upgrade, validate reuse with
the peer's 5-minute `mesh_connection_starts` and source-specific TCP samples; do not use an old
24-hour peak alone to judge the rollout.

### Auth epoch cutover

A multi-node cluster cannot cross to internal-auth v2 through the Web upgrade action. The already
installed v1 `xp-ops` does not recognize the v2 cutover flag, so this boundary requires a
maintenance window and a verified target-release `xp-ops` bootstrap on every host-managed node.
Quiesce the cluster, choose one immutable release tag, and run the following on each node before
allowing any node to resume control-plane traffic:

```bash
export XP_RELEASE=vX.Y.Z
export XP_ARCH="$(uname -m)"
case "$XP_ARCH" in x86_64|amd64) XP_ARCH=x86_64 ;; aarch64|arm64) XP_ARCH=aarch64 ;; *) exit 2 ;; esac
mkdir -p /tmp/xp-internal-auth-v2 && cd /tmp/xp-internal-auth-v2
curl -fLO "https://github.com/IvanLi-CN/xp/releases/download/${XP_RELEASE}/checksums.txt"
curl -fLO "https://github.com/IvanLi-CN/xp/releases/download/${XP_RELEASE}/xp-ops-linux-${XP_ARCH}"
grep " xp-ops-linux-${XP_ARCH}$" checksums.txt | sha256sum -c -
chmod 0755 "xp-ops-linux-${XP_ARCH}"
sudo "./xp-ops-linux-${XP_ARCH}" upgrade --version "$XP_RELEASE" \
  --data-dir /var/lib/xp/data --allow-internal-auth-v2-cutover
```

For the official single-image runtime, pull the target image first, then invoke its marker command
against the same persistent volume before starting that image. Replace the Compose file with the
node's actual bootstrap or join file:

```bash
export XP_IMAGE=ghcr.io/ivanli-cn/xp@sha256:<target-image-digest>
export XP_COMPOSE=deploy/docker/compose.bootstrap.yml
docker compose -f "$XP_COMPOSE" pull xp
docker compose -f "$XP_COMPOSE" run --rm --no-deps --entrypoint xp-ops xp \
  container mark-internal-auth-v2-cutover --data-dir /var/lib/xp/data
docker compose -f "$XP_COMPOSE" up -d xp
```

The marker lives under `${XP_DATA_DIR}/mesh/`. A new binary consumes it into a durable v2 epoch
record; without it startup fails. A host-managed upgrade failure clears a marker it created only
before the new process consumes it. If restart or managed runtime reconcile fails after consumption,
XP stays on v2 while the runtime rollback completes; it never restores the old v1 binary. For a
container failure before the new `xp` process consumes the marker, restore the previous immutable
image and run `container cancel-internal-auth-v2-cutover` through that target image. Once the epoch
record exists, v1 rollback is intentionally unsupported: restore a pre-cutover data backup or
complete the v2 recovery instead. After the durable epoch exists, same-epoch Web upgrades are
available again. Never stagger v1 and v2 nodes outside this maintenance window.

Host-managed upgrade note:

- If no managed-default VLESS endpoint exists, `/etc/xp/xp.env` `XP_DEFAULT_VLESS_PORT` supplies its
  bootstrap port. Once an endpoint exists, its Raft value is authoritative and a stale or changed
  env value does not move it. `XP_DEFAULT_VLESS_SERVER_NAMES` is ignored when the bootstrap port is
  absent; when bootstrap is active, it is validated but does not select SNI.
- If a historical host-managed node has no `XP_DEFAULT_VLESS_*` yet, but the node currently has exactly one legacy VLESS endpoint whose metadata still predates the `managed_default` flag, the new binary auto-adopts that endpoint on startup and rewrites `reality.dest` to the loopback canary only after the canary is healthy; when canary preparation is blocked, startup/sync leave the existing endpoint untouched and surface the error via `vless_https_canary_status`.
- If the node has multiple VLESS endpoints and none are already marked as managed-default, the runtime refuses to guess. In that case the operator must first decide which endpoint should be the managed default before expecting Mihomo relay probing to target that ingress.
- Change an existing managed-default port through the Endpoints Admin UI or
  `PATCH /api/admin/endpoints/{endpoint_id}`. Delete it through the corresponding endpoint delete
  operation. If the bootstrap env remains set after deletion and no same-kind endpoint remains
  available for conservative adoption, the next startup/sync recreates the missing endpoint from
  that value.
- To intentionally rebootstrap at a different port, first change the bootstrap env, explicitly
  delete the endpoint through the Admin UI/API, ensure no other same-kind endpoint can be adopted,
  then restart or run `xp-ops xp sync-node-meta`; ordinary env edits alone never reconfigure an
  existing endpoint.

Deployment note:

- `xp-ops deploy` writes managed-default bootstrap inputs into `/etc/xp/xp.env` when you pass
  `--default-vless-port` + `--default-vless-server-names` and/or `--default-ss-port`; redeploying
  does not override an existing endpoint's Raft-owned port.
- `--ip-geo` explicitly writes `XP_IP_GEO_ENABLED=true`. Omitting it preserves an existing value
  and leaves a new node on XP's disabled default; deploy does not backfill existing nodes.
- `--vless-canary-acme-contact-email` is optional but recommended when you want the VLESS canary certificate flow to be fully operator-owned.
- The host-managed deploy path is therefore no longer container-only; the same one-shot flow now covers host-managed service nodes as well as official single-image container nodes.
- During a fresh single-image join, the wrapper keeps the authenticated XP learner running while
  its initial Raft state is replicated. Managed-default reconcile uses the local XP API as the
  readiness gate and forwards writes through local Raft only after local internal authentication
  observes that replicated state. During the activation deadline it retries only the three expected
  pre-replication signals (`internal sender is not a cluster member`, a missing local leader
  membership record, and `raft client_write forward: leader not available`); unrelated
  authentication, network, and configuration failures still fail immediately.
- Before issuing a fresh join token reservation, the leader verifies that every current voter
  exposes `cluster.join.staged-v1`. Finish the XP rolling upgrade first when the API returns a
  staged-join capability conflict. Host-managed startup keeps managed-default reconciliation active
  until it succeeds after learner replication.

Example host-managed bootstrap (systemd / RHEL-family included):

```bash
sudo -E xp-ops deploy \
  --node-name host-node-1 \
  --access-host edge-node-1.example.net \
  --account-id <cloudflare-account-id> \
  --hostname admin-node-1.example.com \
  --ddns \
  --ip-geo \
  --default-vless-port 443 \
  --default-vless-server-names 'cdn-a.example.test,cdn-b.example.test' \
  --default-vless-fingerprint chrome \
  --default-ss-port 53843 \
  --vless-canary-acme-contact-email ops@example.com \
  --enable-services \
  -y
```

Expected result:

- `/etc/xp/xp.env` contains `XP_DEFAULT_VLESS_*`, `XP_DEFAULT_SS_PORT`, `XP_VLESS_CANARY_*`, and `XP_CLOUDFLARE_DDNS_*`.
- `xp`, `xray`, and optional `cloudflared` are installed and started under the host init system.
- Post-bootstrap relay probing uses `https://<access_host[:managed_vless_port]>/generate_204` instead of the admin origin.

Operational audit:

```bash
# 1. Verify loopback canary runtime state after restart
ssh <alias> 'curl -fsS http://127.0.0.1:62416/api/health | jq .vless_https_canary'
ssh <alias> 'curl -fsS http://127.0.0.1:62416/api/admin/config | jq .vless_https_canary_status'
```

Mesh read-only diagnosis:

- Query `GET /api/admin/mesh/status` on each member and record `mesh_capability`,
  `mesh_reason`, breaker state, current path, and the latest sample timestamp.
- For every directed peer edge, compare the endpoint inventory with the canary status and Xray
  listener, then verify DNS/port reachability and a signed `health-v2` acknowledgement.
- `missing_endpoint`, `ambiguous_endpoint`, and `invalid_access_host` are configuration
  capability failures. `transport_timeout`, `transport_error`, and `protocol_rejected` mean the
  Mesh target exists but the directed transport or protocol failed. A public success with
  `fallback_active` is end-to-end success, not Mesh availability.
- This audit is read-only: do not edit endpoint metadata, restart Xray/XP, reset breakers, or
  remove cluster members as part of diagnosis.

```bash
# 2. Verify loopback TLS canary locally on the node
curl --resolve <access_host>:39043:127.0.0.1 https://<access_host>:39043/generate_204

# 3. Verify live reachability through the managed VLESS ingress port
curl -Ik https://<access_host[:vless_port]>/generate_204
```

## Single-image Docker runtime

If you prefer one container per cluster node, use:

- `docs/ops/docker.md`
- `deploy/docker/compose.bootstrap.yml`
- `deploy/docker/compose.join.yml`

Container-specific note:

- `xp-ops container run` owns the `xray` / `cloudflared` child processes inside the container.
- It also prepares DDNS runtime files and uses container env to bootstrap missing managed SS/VLESS
  endpoints; existing ports and endpoint existence remain cluster-owned.
- For an existing joined node, set `XP_ADMIN_TOKEN_HASH` in the host-owned Compose environment
  file, then recreate the service to rotate the administrator credential. The entrypoint accepts
  only the low-memory Argon2id profile (`m=4096,t=3,p=1`) and atomically reconciles the persisted
  cluster hash before XP starts. A first join retains the leader-provided hash. Do not edit the
  running container or its data volume by hand.
- `xp` still reports `xray` health through `GET /api/health`.
- `cloudflared` is intentionally started outside `xp`'s built-in runtime supervisor, so the Web runtime pages treat `cloudflared` as disabled in container mode.

## `xp-ops mihomo redact` (subscription/config sanitization)

### Mihomo 外部资源公开镜像

Mihomo canonical/provider 订阅可用 `external_resources=mirror` 临时启用 XP 外部资源镜像。镜像目录只来自当前 profile 的 GeoX、rule-provider、proxy-provider HTTPS URL 与固定 MetaCubeX GeoX 资产，不接受任意 `url` 参数。XP 采用 DIRECT 上游、逐块流式转发，不缓存内容、不写磁盘，限制为 256 MiB、90 秒、全局 32 条/单资源 4 条；删除最后一个 profile 引用后资源 ID 立即返回 404。

集群级 `Service config` 中的 `Allow private Mihomo mirror targets` 默认关闭。关闭时，XP 会在初始请求和每次重定向前解析并固定目标地址，只允许公网 IP；loopback、私网、链路本地、保留和文档地址返回 `403 private_target_blocked`。该开关通过 `/api/admin/mihomo/resource-policy` 走 Raft 持久化，只有明确维护可信内部资源时才应开启。

资源端点错误合同：DNS/连接/TLS 为 502，超时为 504，第六次重定向为 508，上游 4xx/5xx 保留状态码但使用脱敏错误体；并发超额为 429。初始资源必须是无 userinfo 的 HTTPS，镜像不转发自定义 Header。

Use `xp-ops mihomo redact` to sanitize Mihomo subscription/config text before sharing logs or snippets.

Command shape:

```bash
xp-ops mihomo redact [SOURCE] [--level minimal|credentials|credentials-and-address] [--source-format auto|raw|base64|yaml] [--timeout-secs N]
```

Behavior:

- `SOURCE` starts with `http://` or `https://`: fetch from URL and sanitize response text.
- `SOURCE` is provided but not URL: read as local file path and sanitize.
- `SOURCE` is `-`: read from stdin and sanitize.
- `SOURCE` omitted: read from stdin and sanitize.
- If both stdin and `SOURCE` are present, `SOURCE` wins.
- Default level is `credentials`; default source format is `auto`; default timeout is 15 seconds.
- Base64 subscription input is decoded, sanitized, and printed as readable plain text.

Script alias:

```bash
./scripts/mihomo-redact.sh [SOURCE] [args...]
```

Quick examples:

```bash
# Local file
xp-ops mihomo redact ./config.yaml

# Explicit stdin with SOURCE='-'
cat ./config.yaml | xp-ops mihomo redact -

# URL source with custom timeout
xp-ops mihomo redact "https://example.com/sub?token=..." --timeout-secs 30
```

## `xp-ops tui` (deploy wizard)

`xp-ops tui` provides an interactive deploy wizard for `xp-ops deploy`.

Note:

- The TUI assumes `xp` is already installed at `/usr/local/bin/xp` (e.g., via `scripts/install-from-github.sh`).
- The TUI covers the same host-managed managed-default inputs as `xp-ops deploy`, including
  `XP_DEFAULT_VLESS_*`, `XP_DEFAULT_SS_PORT`, `XP_VLESS_CANARY_ACME_CONTACT_EMAIL`, and the
  explicit `ip_geo_enabled` switch.
- Turning on `ip_geo_enabled` writes `XP_IP_GEO_ENABLED=true`; leaving it off preserves any
  existing env value and leaves a new node on XP's default-disabled behavior.

Persistence:

- Deploy settings are stored at `/etc/xp-ops/deploy/settings.json`.
- Cloudflare API token is stored at `/etc/xp-ops/cloudflare_tunnel/api_token`.
  - The TUI never prints the token value; it shows `(saved)` or a mask.
  - Leaving the token input empty keeps the existing token unchanged (does not delete or overwrite it).

Key bindings:

- Focus: `Tab` / `Shift+Tab`, `↑` / `↓`, mouse left click
- Editing: type directly into the focused field (use `Backspace` to delete; paste supported)
- Toggles: `Space` (or `Enter`) on boolean fields
- Commands:
  - `Ctrl+S`: save settings (and token if non-empty)
  - `Ctrl+D`: autosave, then deploy (autosave also runs in `dry_run`)
  - `Ctrl+Q`: quit (asks to save if there are unsaved changes)

Quit confirmation (only when there are unsaved changes):

- `Ctrl+S`: save and exit
- `Ctrl+Q`: exit without saving
- `Esc` / `Enter`: cancel

## Environment variables

These names and defaults are sourced from `src/config.rs`.

Required (or commonly set):

- `XP_DATA_DIR` (default: `./data`)
  - Path to the node data directory. See layout below.
- `XP_ADMIN_TOKEN` (default: empty string)
  - Optional bearer token for admin endpoints. Leaving it empty effectively disables token checks.
  - If you bootstrap via `xp-ops deploy`, the plaintext token is printed once for the operator, while the server stores only `XP_ADMIN_TOKEN_HASH` in `/etc/xp/xp.env`.
    - Show the current configured state on the server: `sudo xp-ops admin-token show` (or `--redacted`).
- `XP_ADMIN_TOKEN_HASH` (optional)
  - Argon2id PHC for the administrator credential. New host-managed writes and joined
    container reconciles require `m=4096,t=3,p=1`.
  - For Docker/Compose, place the PHC only in the host-owned Compose environment file, then
    recreate the service through Compose. Do not place the plaintext token in the Compose file.
- `XP_XRAY_API_ADDR` (default: `127.0.0.1:10085`)
  - Address of the local `xray` gRPC API.
- `XP_XRAY_HEALTH_INTERVAL_SECS` (default: `2`, allowed range `1..=30`)
  - Probe interval for `xray` gRPC availability.
- `XP_XRAY_HEALTH_FAILS_BEFORE_DOWN` (default: `3`, allowed range `1..=10`)
  - Consecutive probe failures before reporting `xray.status=down`.
- `XP_XRAY_RESTART_MODE` (default: `none`)
  - `none|systemd|openrc`. When enabled, `xp` requests an init-system restart after `xray` is marked down.
- `XP_XRAY_RESTART_COOLDOWN_SECS` (default: `30`, allowed range `1..=3600`)
  - Minimum time between restart requests (prevents restart storms).
- `XP_XRAY_RESTART_TIMEOUT_SECS` (default: `5`, allowed range `1..=60`)
  - Timeout for the restart command invocation.
- `XP_CLOUDFLARED_HEALTH_INTERVAL_SECS` (default: `5`, allowed range `1..=60`)
  - Probe interval for cloudflared service status (`systemctl is-active` / `rc-service status`).
- `XP_CLOUDFLARED_HEALTH_FAILS_BEFORE_DOWN` (default: `3`, allowed range `1..=10`)
  - Consecutive failures before reporting `cloudflared=down`.
- `XP_CLOUDFLARED_RESTART_MODE` (default: `none`)
  - `none|systemd|openrc`. `none` means cloudflared is treated as disabled in runtime pages.
- `XP_CLOUDFLARED_RESTART_COOLDOWN_SECS` (default: `30`, allowed range `1..=3600`)
  - Minimum time between cloudflared restart requests.
- `XP_CLOUDFLARED_RESTART_TIMEOUT_SECS` (default: `5`, allowed range `1..=60`)
  - Timeout for cloudflared restart command invocation.
- `XP_CLOUDFLARED_SYSTEMD_UNIT` / `XP_CLOUDFLARED_OPENRC_SERVICE`
  - Init-system target names for cloudflared restart/probe.
- `XP_CLOUDFLARE_DDNS_ENABLED` (default: `false`)
  - Enables runtime DDNS reconciliation for `XP_ACCESS_HOST`.
- `XP_CLOUDFLARE_DDNS_TOKEN_FILE` (default: `/etc/xp/cloudflare_ddns_api_token`)
  - Path to the Cloudflare API token file that `xp` can read at runtime.
- `XP_CLOUDFLARE_DDNS_ZONE_ID` (default: empty)
  - Optional explicit Cloudflare zone id. When empty, `xp` derives the zone from `XP_ACCESS_HOST`.
- `XP_CLOUDFLARE_DDNS_IPV4_URL` / `XP_CLOUDFLARE_DDNS_IPV6_URL`
  - Public IP echo endpoints for IPv4 / IPv6 detection. Defaults to `https://cloudflare.com/cdn-cgi/trace`.
- `XP_CLOUDFLARE_DDNS_INTERVAL_SECS_WITH_MONITOR` (default: `300`, allowed range `30..=3600`)
  - Base DDNS poll interval when cloudflared runtime monitoring is enabled.
- `XP_CLOUDFLARE_DDNS_INTERVAL_SECS_NO_MONITOR` (default: `60`, allowed range `30..=3600`)
  - Base DDNS poll interval when cloudflared runtime monitoring is disabled.
- `XP_CLOUDFLARE_DDNS_FAST_INTERVAL_SECS` (default: `30`, allowed range `10..=600`)
  - Fast-mode DDNS poll interval after cloudflared recovery-style hints.
- `XP_CLOUDFLARE_DDNS_FAST_WINDOW_SECS` (default: `300`, allowed range `30..=3600`)
  - Duration of the fast-mode DDNS polling window.
- `XP_CLOUDFLARE_DDNS_FAMILY_MISSING_GRACE` (default: `3`, allowed range `1..=10`)
  - Consecutive hard-missing observations before deleting an `A` or `AAAA` record.
- `XP_VLESS_CANARY_BIND` (default: `127.0.0.1:39043`)
  - Loopback bind address for the TLS canary used by xp-managed VLESS/REALITY fallback.
- `XP_VLESS_CANARY_ACME_DIRECTORY_URL` (default: `https://acme-v02.api.letsencrypt.org/directory`)
  - ACME directory for DNS-01 certificate issuance.
- `XP_VLESS_CANARY_ACME_CONTACT_EMAIL` (default: empty)
  - Optional ACME contact email.
- `XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE` (default: `/etc/xp/cloudflare_ddns_api_token`)
  - Path to the Cloudflare API token file used for DNS-01 challenges. By default it reuses the same xp-readable runtime token file as DDNS.
- `XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID` (default: empty)
  - Optional explicit Cloudflare zone id for DNS-01; when empty, `xp` first reuses `XP_CLOUDFLARE_DDNS_ZONE_ID` when present, and only falls back to deriving the zone from `XP_ACCESS_HOST` when the DDNS zone is also unset.
- `XP_VLESS_CANARY_DNS_PROPAGATION_TIMEOUT_SECS` (default: `180`)
  - Maximum wait budget for the DNS-01 TXT to become visible through both Cloudflare and Google
    DNS-over-HTTPS resolvers before ACME validation starts. Nodes do not need direct authority
    access on UDP/TCP port 53.

DDNS runtime notes:

- `xp` starts one DDNS probe immediately on startup.
- `xp` only updates Cloudflare when the observed public IP actually changes.
- `cloudflared` is only used as a heuristic fast-mode trigger (`down -> up` / `became available`), never as the source of truth for public IPs.
- Probe timeouts or transient upstream errors do not delete records; only repeated hard evidence of a missing address family can remove `A` / `AAAA`.
- Nodes with only IPv4 connectivity are healthy DDNS targets: IPv6 `network unreachable`, `no route`, unsupported address family, or local address assignment failures are treated as missing IPv6 candidates rather than runtime degradation.

Optional quota knobs:

- `XP_QUOTA_POLL_INTERVAL_SECS` (default: `10`, allowed range `5..=30`)
- `XP_QUOTA_AUTO_UNBAN` (default: `true`)

Optional inbound IP geo knobs:

- `XP_IP_GEO_ENABLED` (default: `false`)
  - When enabled, `xp` resolves newly-seen inbound public IPs via the free `country.is` hosted API.
  - Note: this sends observed client IPs to a third-party service.
  - On host-managed nodes, set it during deployment with `xp-ops deploy --ip-geo` or the TUI
    switch. Omitting either preserves an existing value and does not add the key to a new node
    env file.
- `XP_IP_GEO_ORIGIN` (default: `https://api.country.is`)
  - Override the hosted API origin (e.g. self-hosting the same interface or special network environments).

An example env file is provided at `docs/ops/env/xp.env.example`.

## Inbound IP usage prerequisites

To expose minute-level inbound IP usage in the admin UI, the node must enable Xray online stats. Geo enrichment can optionally use the free `country.is` hosted API (`XP_IP_GEO_ENABLED=true`) and no longer requires local MMDB files or a dedicated Geo settings page.

1. Required: Xray static config enables `statsUserOnline=true` together with the existing traffic stats and reclaim profile (`handshake=4`, `connIdle=300`, `uplinkOnly=2`, `downlinkOnly=5`).
2. When `XP_IP_GEO_ENABLED=true`, nodes need outbound HTTPS access to `https://api.country.is/` so new public IPs can be resolved on first sight.
3. The node egress probe used by Mihomo region auto-grouping also relies on outbound HTTPS access to the public IP trace endpoint (default `https://cloudflare.com/cdn-cgi/trace`) and to `https://api.country.is/`.
4. `xp` caches resolved IP geo/operator fields inside `inbound_ip_usage.json`; API lookup failures only leave the affected fields empty and do not interrupt quota collection (the admin UI will show an `ip_geo_lookup_failed` warning after failed lookups).

Operational notes:

- No local Geo DB download/update job runs anymore, so `${XP_DATA_DIR}/geoip` is not used by the default IP usage pipeline.
- Upgrades from releases that used managed DB-IP geo enrichment must opt in again via `XP_IP_GEO_ENABLED=true`; otherwise `geo_source=missing` and geo fields stay empty.
- `statsUserOnline` is required for the online IP snapshot itself. If it is missing, `xp` keeps quota collection running and returns an `online_stats_unavailable` warning to the admin UI.
- `xp-ops init` now writes `/etc/xray/config.json` with the level-0 reclaim profile and `statsUserOnline=true` by default; nodes provisioned before this change should verify their static config before rollout.

Quick checks on a node:

```
jq '.policy.levels["0"]' /etc/xray/config.json
ls -l "${XP_DATA_DIR}/inbound_ip_usage.json" || true
jq '.online_stats_unavailable' "${XP_DATA_DIR}/inbound_ip_usage.json" 2>/dev/null || true
```

## Node TCP connection history prerequisites

To expose minute-level TCP connection history in the admin UI, the node must run on Linux and allow `xp` to read the local `/proc/net/tcp` and `/proc/net/tcp6` socket tables.

1. Required: the node OS is Linux. Non-Linux platforms return an `unsupported_platform` warning instead of a zero-value chart.
2. Required: business endpoints are configured in xp state with their actual listen `port`, because TCP history maps counts by node-local endpoint port.
3. Scope: only socket-level `ESTABLISHED` inbound TCP connections on business endpoint listen ports are counted.
4. Excluded: `xp` admin port, Xray API, `cloudflared`, and outbound connections are not part of this panel.
5. Storage: xp persists the most recent 7 days of minute samples in `${XP_DATA_DIR}/tcp_connection_usage.json`.

Operational notes:

- The chart aggregates selected endpoints by direct per-minute summation; there is no cross-endpoint deduplication.
- TCP history is independent from Xray `statsUserOnline`; missing online IP stats do not block TCP connection sampling.
- Socket read failures surface as warnings in the admin UI and do not interrupt the quota worker main flow.

Quick checks on a node:

```
uname -s
ls -l "${XP_DATA_DIR}/tcp_connection_usage.json" || true
ss -tn state established '( sport = :443 or sport = :8443 )' || true
head -n 5 /proc/net/tcp
```

## VLESS transport migration (XHTTP/XMUX)

New VLESS Reality endpoints use XHTTP/XMUX to let Mihomo reuse one HTTP/2 TCP transport after the
pool is warm. Existing endpoints deliberately remain on Vision/TCP until an administrator changes
that endpoint's **Advanced: VLESS transport** setting; an XP upgrade alone does not change the
wire transport of existing subscribers.

Before changing an endpoint, upgrade the node to a release that bundles Xray `v26.3.27` or newer
and confirm every affected client runs Mihomo `v1.19.29` or newer. Change one endpoint at a time:

1. Select **XHTTP / XMUX** and save. XP removes and rebuilds that endpoint's Xray inbound, so
   active connections on that endpoint are interrupted briefly.
2. Re-render and refresh the affected YAML subscriptions. The new profile carries `network: xhttp`
   and fixed low-resource XMUX settings; an old Vision/TCP profile cannot connect to the rebuilt
   inbound.
3. Observe new connection samples after the historical chart window has rolled forward. The TCP
   chart still combines business traffic and Mesh traffic, so use the endpoint's new steady-state
   samples rather than an old 24-hour peak as the migration verdict.

To roll back, select **Vision TCP**, save, and refresh the same clients again. Do not switch every
endpoint simultaneously: verify one subscriber path first, then proceed endpoint by endpoint.

## Data directory layout (`XP_DATA_DIR`)

The runtime persists its identity, raft state, and snapshots under `XP_DATA_DIR`. This layout matches the code in:

- `src/cluster_metadata.rs`
- `src/raft/node.rs`
- `src/state.rs`

```
${XP_DATA_DIR}/
  cluster/
    metadata.json
    cluster_ca.pem
    cluster_ca_key.pem
    node_cert.pem
    node_key.pem
    node_csr.pem
  raft/
    wal/
    snapshots/
  state.json
  usage.json
  history.sqlite3
  node_history_cache.json
  inbound_ip_usage.json
  service_runtime.json
  ddns_state.json
```

Notes:

- `cluster/` holds long-lived identity and TLS assets. Treat `cluster_ca_key.pem` as sensitive (private key).
- `raft/` holds the raft write-ahead log and snapshots.
- `state.json` and `usage.json` are raft-backed JSON snapshots; on schema mismatches, startup fails instead of silently migrating.
- `history.sqlite3` is the local repository replica database. SQLite uses WAL and bounded
  checkpoints with incremental page release; XP never runs an unbounded `VACUUM` in the service
  path. Persistent low disk space or a repository quota stops new history writes while Raft and
  normal control-plane operations remain available. Operators can inspect member capacity,
  coverage, watermarks, gaps, clock skew and `complete` / `partial` / `local_only` query quality
  through the admin repository endpoints; requests have a bounded range, page size and cursor, so
  the endpoints are not an arbitrary SQL or bulk-export interface.
- `inbound_ip_usage.json` is a local-only high-frequency store for inbound IP presence (7-day retention, 1-minute bitmap window, Geo cache). It is **not** replicated via raft.
- `node_history_cache.json` stores local node history and Traffic analytics. Traffic sampling runs on UTC five-minute boundaries and writes the same Xray counter delta to node and real-user rollups. It retains at most 588 five-minute buckets (49 hours) and 90 UTC daily buckets; hourly rollups are not stored. Endpoint probe traffic is included in node totals but is never exposed as a normal user.
- Missing samples, first tracking, and counter resets remain partial and are surfaced as warnings; operators must not treat gaps as zero traffic. Deleting a user clears its stored history, deleting a node clears its node and user-node history, and removed memberships expire naturally with the retention windows. These node-data retention semantics are unchanged by the repository storage-medium migration.
- `service_runtime.json` stores local runtime status/event history used by `/api/admin/nodes/*/runtime` views (7-day window, local node only).
- `ddns_state.json` stores local Cloudflare DDNS reconcile state (last synced IPs, record ids, error state, fast-mode window). It is **not** replicated via raft.
- Geo enrichment uses a hosted API (`https://api.country.is/`); there are no local Geo DB files under `XP_DATA_DIR`.

## Service examples

### systemd

See:

- `docs/ops/systemd/xp.service`
- `docs/ops/systemd/xray.service`
- (optional) `docs/ops/systemd/cloudflared.service`

Recommended workflow:

1. Copy the unit files to `/etc/systemd/system/`.
2. Copy `docs/ops/env/xp.env.example` to `/etc/xp/xp.env` and edit as needed.
3. Ensure `XP_DATA_DIR` exists and is writable by the service user; this includes the local
   `history.sqlite3` repository database when the node is configured as a repository.
4. Enable and start services:

```
sudo systemctl daemon-reload
sudo systemctl enable --now xray.service
sudo systemctl enable --now xp.service
```

### OpenRC (Alpine-like)

See:

- `docs/ops/openrc/xp`
- `docs/ops/openrc/xray`
- (optional) `docs/ops/openrc/cloudflared`

Suggested workflow:

1. Copy scripts to `/etc/init.d/` and make executable.
2. (Optional) Configure environment variables via OpenRC's `/etc/conf.d/<service>` mechanism.
   Keep `XP_DATA_DIR` writable by the `xp` service user so the repository SQLite database survives
   restart.
3. Add to default runlevel and start:

```
sudo rc-update add xray default
sudo rc-update add xp default
sudo rc-service xray start
sudo rc-service xp start
```

### Cloudflare Tunnel transport

Managed cloudflared services default to `--protocol http2`. This avoids
startup stalls on networks that block outbound QUIC/7844. Override only when
the local network has verified QUIC support:

- systemd: add `Environment=XP_CLOUDFLARED_PROTOCOL=quic` in a separate
  `cloudflared.service.d/` drop-in.
- OpenRC: set `XP_CLOUDFLARED_PROTOCOL=quic` in `/etc/conf.d/cloudflared`.
- containers: pass `XP_CLOUDFLARED_PROTOCOL=quic` to `xp-ops container run`.

Managed cloudflared also defaults to `GOMEMLIMIT=12MiB`, `GOGC=50`, and
`TUNNEL_MANAGEMENT_DIAGNOSTICS=false`. Existing XP-generated `8MiB` defaults
in complete managed templates are upgraded to `12MiB`; explicit operator
overrides remain unchanged. Upgrade inspection includes systemd drop-ins under
`/usr/lib`, `/usr/local/lib`, `/run`, and `/etc`, including same-name masking by
higher-priority directories. If a lower-priority directory already owns the
managed filename, XP leaves it active instead of masking it. `EnvironmentFile`
paths are normalized without escaping the managed root. Legacy custom/provider OpenRC scripts have no
durable ownership marker, so an existing `8MiB` value in those scripts is
preserved and must be changed explicitly after operator review. The exact
known provider wrapper is migrated only when it is a regular file; symbolic
links and other custom filesystem objects remain untouched.

## Upgrade and rollback strategy

### Recommended: upgrade via `xp-ops` (GitHub Releases)

`xp-ops` can upgrade both `xp` and `xp-ops` from GitHub Releases (Linux musl assets).

Upgrade both `xp` and `xp-ops`:

```
sudo xp-ops upgrade --version latest
```

Current rollout semantics:

- `xp-ops upgrade` first locks the target release.
- It upgrades `xp`, installs the checksummed release-managed Xray and cloudflared pair when present, rewrites `/etc/xray/config.json` to the current static baseline, and restarts both services before replacing `xp-ops` itself.
- Deferring the `xp-ops` replacement prevents a self-update from ending the locked release phase before the service binaries and managed runtimes are updated.
- A service restart is accepted only after systemd reports the unit active or OpenRC reports the
  service started. This prevents an asynchronous OpenRC transition from being reported as a
  completed upgrade.
- During static config rewrite, `xp-ops upgrade` preserves the authoritative `XP_XRAY_API_ADDR` binding for the `api` inbound and removes the retired legacy control-plane proxy inbound when present.
- If runtime installation, configuration reconciliation, or either service restart fails, `xp-ops upgrade` restores the previous runtime pair and `xp`; Xray config and a self-upgraded `xp-ops` are also restored when applicable.

Useful flags:

- `--dry-run` prints the resolved release + actions without downloading/writing/restarting.
- `--prerelease` (only with `--version latest`) selects the newest prerelease instead of stable.
- `--repo <owner/repo>` (or `XP_OPS_GITHUB_REPO=<owner/repo>`) overrides the default source repo.

UI notes:

- The Web UI header shows the current `xp` version (clickable) and can check whether a newer stable GitHub Release exists. Automatic focus checks may use the node's short-lived latest-release cache; the popover's manual Check bypasses that cache.
- On host-managed systemd/OpenRC nodes, the Web UI can start a local in-place upgrade for the
  current node after admin confirmation. The actual upgrade is still performed by `xp-ops upgrade`
  through a restricted one-shot root runner.
- Docker / Compose nodes do not support in-container Web automatic upgrade. Upgrade them from the
  host by changing the image tag or digest and restarting the container.
- If you override the upgrade source repo via `XP_OPS_GITHUB_REPO`, the version check uses the same repo.
  The Web start API also uses this server-side repo setting and ignores browser-supplied repo
  overrides.

Web-triggered local upgrade contract:

- `xp` exposes admin-only `GET /api/admin/upgrade/status` and `POST /api/admin/upgrade/start`.
- The start request must include the confirmed release tag, for example `v0.3.0`; `latest` is
  intentionally not accepted by the Web start API.
- `xp` writes the restricted request to `${XP_DATA_DIR}/upgrade/request.json` and records durable
  status at `${XP_DATA_DIR}/upgrade/status.json`.
- Only one active job is allowed. A second start request while a job is `running` or `restarting`
  returns `409 upgrade_already_running`. The short start critical section is protected by an
  advisory lock on `${XP_DATA_DIR}/upgrade/start.lock`; the file may persist, but only a live lock
  holder blocks a request. The lock is released before the host trigger runs.
- The Web UI starts a same-tab observation before it sends start and polls status every 2.5 seconds.
  If the restart boundary returns an unstructured 5xx or drops the connection, that response is
  treated as unknown rather than failed; the client keeps observing the durable status for up to 60
  seconds and restores the remaining window after a same-tab refresh.
- During observation the popover stays visible, Upgrade remains disabled, and the header keeps a
  spinner. The operator may close the popover with outside click or Esc without cancelling the
  observation. A terminal status (`succeeded`, `failed`, or `unsupported`) ends it; a 60-second
  timeout stops automatic polling and keeps Upgrade locked until a manual Status query finds an
  active job (new window) or an idle/terminal state (unlock).
- After `409 upgrade_already_running`, the Web client refreshes status immediately. A current
  `running` / `restarting` job continues through the normal observation window. If the node reports
  only an older terminal status or idle, the client shows a stale-conflict error and unlocks
  Upgrade immediately.
- If a host one-shot runner fails before it can write a terminal status, the status endpoint
  reconciles the stale active status to `failed` instead of reporting `running` forever. On systemd
  nodes this reconciliation uses `xp-upgrade.service` failure state as the durable local fact; on
  OpenRC nodes it uses the `xp-upgrade` crashed state.

Host-managed root delegation:

- systemd nodes use `xp-upgrade.service` as a root one-shot service. `xp-ops init` writes a
  root-owned fixed helper at `/usr/local/libexec/xp-upgrade-trigger` plus
  `/etc/sudoers.d/91-xp-upgrade`, allowing the `xp` user to run only that helper and its no-op
  `--check` probe. The helper starts only `xp-upgrade.service`.
- The systemd one-shot unit must call `/usr/local/bin/xp-ops _upgrade-runner` directly.
  `XP_DATA_DIR` is supplied by `Environment=XP_DATA_DIR=...` plus `/etc/xp/xp.env`, so the unit must
  not wrap the command in `/bin/sh -c` just to pass `--data-dir "${XP_DATA_DIR:-...}"`. systemd can
  consume that shell expression before the runner sees it.
- `xp-ops init` also writes a narrow systemd polkit rule for hosts whose polkit exposes `unit` and
  `verb` action details. CentOS 7-class polkit does not expose those details reliably, so Web
  upgrade support must not depend on the polkit rule alone.
- OpenRC nodes use `xp-upgrade` as a root one-shot service. `xp-ops init` writes the root-owned
  fixed `/usr/local/libexec/xp-openrc-upgrade-trigger` helper and appends two narrow doas rules:
  `xp` may run only the helper's `--check` and `start` actions. The helper verifies the executable
  runner and exact rules as root; `start` zaps only a crashed fixed service before starting it.
  The service backgrounds the runner, records its PID, and zaps its OpenRC state after every exit.
  This lets the service verify a root-owned `0600` `/etc/doas.conf` without granting it read access
  and prevents completed one-shots from remaining started or crashed.
  On an existing node that predates this helper, run `sudo xp-ops init` once after the ordinary
  `sudo xp-ops upgrade --version latest` has completed. The old running `xp-ops` cannot execute
  release code that it has not installed yet, so this root-approved reinitialization is required;
  readiness checks must not start the one-shot service as an implicit migration.
- When recovering a node created by the older direct-`rc-service` contract, first prove the durable
  status is terminal and no `_upgrade-runner` or transaction lock is live. Then remove only the
  exact stale `start.lock`, run `rc-service xp-upgrade zap`, perform one normal Web upgrade as a
  canary, and run the upgraded `xp-ops init` to backfill the helper and doas policy. Require the full
  node audit to pass before applying the recovery to any additional node.
- Reference samples live at:
  - `docs/ops/systemd/xp-upgrade.service`
  - `docs/ops/systemd/xp-upgrade-trigger`
  - `docs/ops/systemd/sudoers-xp-upgrade`
  - `docs/ops/systemd/xp-upgrade.polkit.rules`
  - `docs/ops/openrc/xp-upgrade`
  - `docs/ops/openrc/xp-upgrade-trigger`
  - `docs/ops/openrc/doas-xp-upgrade.conf`

Rollback notes:

- Before every host-managed upgrade, `xp-ops` removes only its own old regular-file artifacts:
  `.bak.*`, `.failed.*`, and staged files for `xp`, `xp-ops`, Xray, and cloudflared, plus the exact
  `/tmp/xp-ops` work directory. It never follows symlinks and never cleans `/var/backups/xp`,
  configuration, credentials, certificates, or Raft data.
- All host-managed nodes use the same retention contract, regardless of disk size. A backup exists
  only for the active replacement transaction. After a successful upgrade or a successful rollback,
  no `.bak.*` or `.failed.*` binary remains. There is no persistent local binary fallback.
- If the filesystem prevents restoring a transaction backup, `xp-ops` returns `rollback_failed` and
  preserves that affected `.bak.*` file for manual recovery. It never deletes the only known old
  binary merely to reach the normal zero-artifact terminal state. A later upgrade is rejected while
  that preserved backup remains; recover it first, then rerun the upgrade.
- Upgrades require at least `128 MiB` available on both the installation and download-workspace
  filesystems after that managed cleanup. The Web status endpoint reports the current available and
  reclaimable bytes; the root runner performs the authoritative check before downloading anything.
- A failed upgrade records only the latest `${XP_DATA_DIR}/upgrade/diagnostics.json` (release tag,
  asset SHA-256 values, phase, exit code, and a bounded error summary). A later successful upgrade
  deletes that diagnostic file.
- On `xp` restart failures, `xp-ops upgrade` automatically rolls back to the previous `xp` binary.
- On managed-runtime or restart failures, `xp-ops upgrade` restores the previous Xray/cloudflared binaries, Xray config, and `xp` before returning failure.
- If that failure happened after a self-upgrade re-exec, `xp-ops upgrade` also restores the previous `xp-ops` binary instead of leaving the node on the newer operator binary.

### Deployment-specific upgrade paths

Use the path that matches the node shape instead of mixing procedures:

- Host-managed systemd/OpenRC nodes:
  - Upgrade binaries with `xp-ops upgrade` when the distro family is officially supported by `xp-ops`.
  - At the internal-auth v2 boundary, use the verified bootstrap procedure above instead of an
    ordinary `xp-ops upgrade` invocation from the old installed binary.
  - Alternatively, after `xp-ops init` has installed the one-shot runner and narrow privilege rule,
    start the same current-node upgrade from the Web UI.
  - Arch/Debian/Ubuntu/RHEL-family nodes are covered by the supported automation path.
  - If a host-managed node falls outside those distro families, upgrade the `xp` and `xp-ops` binaries manually, then restart `xp` and verify the post-upgrade checks below.
- Docker / Compose nodes:
  - Update the image tag or digest, then restart the container.
  - At the internal-auth v2 boundary, use the image marker procedure above; the fixed entrypoint
    cannot receive a new `container run` flag from Compose.
  - Let `xp-ops container run` perform runtime reconcile on startup.
  - The Web UI reports this shape as unsupported for automatic upgrade and does not replace binaries
    inside the container.

Post-upgrade validation for nodes expected to expose a managed-default VLESS ingress:

1. `curl -fsS http://127.0.0.1:62416/api/admin/config | jq .vless_https_canary_status`
2. `curl -Ik https://<access_host[:vless_port]>/generate_204`
3. Re-render a Mihomo provider subscription and confirm the relay group for that `access_host` now uses `https://<access_host[:port]>/generate_204`

### Release-ready checklist: host-managed systemd node with Tunnel/DDNS

Ideal post-release path:

1. Install/upgrade `xp-ops` and `xp` on the node with the standard host-managed path.
2. Run `xp-ops deploy` with:
   - `--node-name`
   - `--access-host`
   - `--account-id`
   - `--hostname` when Tunnel is enabled
   - `--ddns` when `XP_ACCESS_HOST` should be maintained by Cloudflare
   - optional `--ip-geo` when inbound client IPs may be sent to `https://api.country.is`
   - `--default-vless-port`
   - `--default-vless-server-names`
   - optional `--default-vless-fingerprint`
   - optional `--default-ss-port`
   - recommended `--vless-canary-acme-contact-email`
   - `--enable-services -y`
3. Confirm `/etc/xp/xp.env` contains the intended bootstrap endpoint keys and the canary/DDNS keys.
   Existing endpoint ports may intentionally differ from stale bootstrap values.
4. Restart validation:
   - `curl -fsS https://<api-base-url>/health` returns HTTP `200` for a joined node with public
     access
   - `curl -fsS http://127.0.0.1:62416/api/admin/config | jq .vless_https_canary_status`
   - `curl -Ik https://<access_host[:vless_port]>/generate_204`
   - render a Mihomo provider subscription and confirm the relay URL points at the managed VLESS ingress
5. If the node was an older single-VLESS deployment without `XP_DEFAULT_VLESS_*`, verify that startup auto-adopted the lone endpoint only when its metadata still predates the `managed_default` flag.
6. If the node has multiple legacy VLESS endpoints and no managed-default marker, stop and choose the owner-facing default explicitly before expecting Mihomo relay probing to switch over.

### Release-ready checklist: official single-image container node

Ideal post-release path:

1. Update the image tag/digest for the official single-image runtime.
2. Ensure the container env includes:
   - `XP_NODE_NAME`
   - `XP_ACCESS_HOST` when the node has public ingress
   - `XP_CLOUDFLARE_DDNS_ENABLED=true` when DDNS should manage `XP_ACCESS_HOST`
   - `XP_DEFAULT_VLESS_PORT` when a missing VLESS endpoint should be bootstrapped
   - `XP_DEFAULT_VLESS_SERVER_NAMES`
   - optional `XP_DEFAULT_VLESS_FINGERPRINT`
   - optional `XP_DEFAULT_SS_PORT` when a missing SS2022 endpoint should be bootstrapped
   - optional `XP_VLESS_CANARY_ACME_CONTACT_EMAIL`
3. Restart the container so `xp-ops container run` replays bootstrap/join, runtime reconcile, and
   missing endpoint bootstrap without changing existing cluster-owned ports.
4. Validate:
   - container logs show successful `xp-ops container run`
   - `GET /api/admin/config` returns healthy `vless_https_canary_status`
   - `curl -Ik https://<access_host[:vless_port]>/generate_204` succeeds from outside the node path you actually use
   - Mihomo provider render uses `https://<access_host[:managed_vless_port]>/generate_204`
5. To change a managed-default port or remove an endpoint, use the Admin UI/API. Editing or removing
   bootstrap env alone has no effect on an existing endpoint. Retaining the env after an API
   deletion recreates the missing endpoint on next reconcile when no same-kind endpoint remains
   available for conservative adoption.

## Disaster recovery: quorum lost (single-node leader recovery)

Raft has three role outcomes: `voter`, transient `learner`, and `absent`. Long-lived learners,
observer nodes, `can_vote` flags, or `voter=false` configuration are not supported. Every voter
must map to a DesiredState Node. A 2-voter topology is not an acceptable production shape because
losing either voter removes writable quorum; use at least 3 stable voters for production clusters.

Before fresh join, restore, delete, or repair, upgrade every voter to a build that exposes
`cluster.membership-lifecycle-v1`. Upgrade one voter at a time and preserve serving quorum. During
the mixed-version interval lifecycle writes return `coordinated_upgrade_required`; do not work
around that freeze by calling internal membership APIs or editing Raft files.

### Orphan voter incident

An orphan voter is a current voter without a DesiredState Node mapping. It is not a learner and it
must never be converted or removed by a periodic scan. First finish the rolling upgrade barrier,
confirm the target is the only orphan and that the local node is leader, then run the local
signed-CLI dry-run:

```bash
sudo xp-ops xp repair-orphan-voter --api-base-url http://127.0.0.1:62416 --raft-node-id <id>
```

Copy the returned `expected_membership` exactly into the explicit apply command:

```bash
sudo xp-ops xp repair-orphan-voter --api-base-url http://127.0.0.1:62416 --raft-node-id <id> \
  --apply --expected-membership <fingerprint>
```

The repair removes only that non-leader voter with `RemoveVoters(..., false)`. It does not touch
DesiredState Nodes, endpoints, users, traffic configuration, or raw Raft files. If the fingerprint,
leader, joint-consensus, mapping, session, or operation precondition changes, stop and investigate;
do not retry with `recover-single-node` or a bulk repair. After the target is absent, verify the
membership invariant and perform a normal fresh join when the node should return.

If quorum is permanently lost, the surviving healthy node cannot elect a leader by itself. In this
case you can force a single-node Raft membership on the chosen surviving node to restore write
availability. Failed or offline nodes are not kept as learners during this recovery; repair them
separately and join them again after the recovered leader is writable.

Warning:

- This is an unsafe recovery procedure. Any committed state that existed only on the wiped node is
  lost permanently.
- This rewrites local Raft persistence on disk. Stop `xp` before running it.

Procedure (surviving node):

1. Choose the surviving node with the most recent healthy data. Do not run recovery on multiple
   nodes.
2. Stop `xp` on the chosen node (systemd/OpenRC).
3. Run a dry-run first:

```
sudo xp-ops xp recover-single-node --dry-run
```

4. Run the recovery command:

```
sudo xp-ops xp recover-single-node -y
```

Notes:

- By default, `xp-ops` creates a backup copy at `${XP_DATA_DIR}/raft.bak-<timestamp>`. You can skip
  it with `--no-backup` (not recommended).
- After restart, leader election may take up to ~6-12 seconds (WAN-tuned defaults).
- Do not manually edit Raft membership files and do not try to preserve offline nodes as learners.

After recovery:

- Start `xp` on the recovered node and wait until `/api/cluster/info` reports `"role": "leader"`.
- Confirm an admin write works, for example by creating a temporary endpoint and then deleting it.
- Re-join each repaired node using a join token issued by the recovered leader
  (`/api/admin/cluster/join-tokens`), then run `xp join` on the repaired node and restart its
  service.
- `xp join` success means the node's authenticated bootstrap material is durable and the learner
  can start. The leader promotes it only after Raft catch-up; do not consider deployment complete
  until membership reports the node as a voter and its public health endpoint returns `200`.
- Re-running `xp join` after a lost response reuses the pending node key/CSR and the same reserved
  token. Retries do not extend the fixed 10-minute activation deadline. If the learner never starts,
  the leader removes the incomplete membership/node and expires the reservation.
- Run `xp-ops xp sync-node-meta` on each node after updating `/etc/xp/xp.env` to ensure membership
  `NodeMeta` (leader discovery/forwarding) matches config. This command preserves existing
  managed-default endpoint ports from Raft even when the local bootstrap env differs.
- After all intended nodes are rejoined, confirm `/api/cluster/info` has a leader and endpoint
  creation succeeds through the admin UI/API. Any unexpected learner or voter without a DesiredState
  mapping is an incident. The periodic worker reports it but never repairs it; use the bounded
  orphan-voter runbook above only when its preconditions hold, otherwise use explicit disaster
  recovery.

### Backup before upgrade

Before upgrading the binary, stop the service and back up the entire `XP_DATA_DIR`. The most critical parts are:

- `cluster/` (identity + CA material)
- `raft/` (wal + snapshots)

Example:

```
sudo systemctl stop xp.service
sudo tar -C "$(dirname "$XP_DATA_DIR")" -czf "xp-data-$(date +%Y%m%d%H%M%S).tgz" "$(basename "$XP_DATA_DIR")"
```

### Upgrade steps

1. Stop `xp`.
2. Back up `XP_DATA_DIR`.
3. Deploy the new `xp` binary (and restart).

If `xp` starts cleanly, the upgrade is complete.

### What to do on startup failures / schema mismatches

`xp` validates on-disk schema versions and fails fast on mismatches for:

- `cluster/metadata.json` schema version (`src/cluster_metadata.rs`)
- `state.json` schema version (`src/state.rs`)
- `usage.json` schema version (`src/state.rs`)

If you see startup failures mentioning schema/version mismatch, do not edit these files manually. The safe recovery path is:

1. Stop `xp`.
2. Roll back to the previous `xp` binary (the last known-good version).
3. Restore the `XP_DATA_DIR` backup you took before the upgrade.
4. Start `xp` again.

### `xp init` compatibility check (high-level)

`xp init` initializes `cluster/metadata.json` and then loads/initializes `state.json` using the new node identity.
If `state.json` already exists but does not contain exactly one node matching the new `metadata.json` node ID,
`xp init` fails with a compatibility error.

Practical guidance:

- Do not re-run `xp init` against an existing data dir unless you are intentionally bootstrapping a new cluster.
- For an existing node, upgrade by swapping the binary and keeping the existing `XP_DATA_DIR` (with a backup).
