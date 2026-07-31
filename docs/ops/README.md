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
| Single-image container node | Docker Compose / OCI runtime | fully supported | official single-image container node                                  | `xp-ops container run` owns bootstrap/join, child process supervision, and default endpoint reconcile |

Current support boundaries that operators must know:

- Host-managed automation in `xp-ops` currently recognizes Arch/Debian/Ubuntu/RHEL-family/Alpine distro families. Historical CentOS 7 / RHEL-family host-managed nodes are first-class host-managed targets and should use the host-managed deployment / upgrade paths in this document.
- Feature delivery must not be container-only. Runtime contracts such as managed-default endpoint reconcile, VLESS HTTPS canary fallback, Mihomo relay URL generation, and upgrade-time auto-adoption must behave the same way once a node is running, regardless of whether the node is host-managed or container-managed.
- When a deployment environment needs manual intervention, document the exact branch and operator steps instead of implying the generic path will work.

## Minimal runtime assumptions

Host-managed mode assumptions:

- `xp` runs as a local HTTP admin/API server and binds loopback by default (`127.0.0.1:62416`).
- `xray` runs locally and exposes its gRPC API on loopback by default (`127.0.0.1:10085`).
- `xp` talks to `xray` via gRPC at `XP_XRAY_API_ADDR`.
- `xp` can optionally route xp-to-xp control-plane HTTP requests through a local proxy with `XP_MESH_PROXY_URL`; `xp-ops init` provisions a loopback-only Xray SOCKS listener at `127.0.0.1:10808` for this purpose.
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

## Optional: managed VLESS HTTPS canary

If you want Mihomo relay `url-test` to probe the actual managed VLESS ingress instead of the admin API origin, configure the loopback TLS canary:

- `XP_VLESS_CANARY_BIND=127.0.0.1:39043` by default.
- `XP_VLESS_CANARY_ACME_DIRECTORY_URL` defaults to Let's Encrypt production.
- `XP_VLESS_CANARY_ACME_CONTACT_EMAIL` is optional but recommended.
- `XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE` defaults to `/etc/xp/cloudflare_ddns_api_token` so host-managed nodes can reuse the same xp-readable Cloudflare runtime token as DDNS.
- `XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID` is optional; when empty, `xp` first reuses `XP_CLOUDFLARE_DDNS_ZONE_ID` when present, and only falls back to deriving the Cloudflare zone from `XP_ACCESS_HOST` when the DDNS zone is also unset.
- `XP_VLESS_CANARY_DNS_PROPAGATION_TIMEOUT_SECS` defaults to `180`; `xp` waits until the DNS-01 TXT is visible on every authoritative nameserver for the zone before asking the ACME server to validate it.

Contract:

- `xp` terminates TLS for `GET/HEAD /generate_204` on the loopback canary and returns `204`.
- xp-managed/default VLESS/REALITY endpoints set `reality.dest` to that loopback canary, and set `server_names` to `[XP_ACCESS_HOST]` without a port. `XP_DEFAULT_VLESS_SERVER_NAMES` is deprecated compatibility input; when present it is validated, but it does not choose managed VLESS SNI.
- The canary routes non-probe HTTPS traffic by HTTP authority, not by TLS SNI. `Host` / HTTP/2 `:authority` always accepts the canonical `XP_ACCESS_HOST[:endpoint_port]`; `:443` may be omitted. Managed VLESS endpoints may also carry an extra `accepted_authorities` set of normalized `host[:port]` aliases; omitting the port means HTTPS default `443`. Exactly one managed VLESS endpoint on the node must match the canonical authority or one of those aliases.
- Each managed VLESS endpoint may store its own `canary_upstream` origin URL and `accepted_authorities` alias set. `accepted_authorities` only affects ordinary HTTPS Host matching; it does not change REALITY `server_names`, `reality.dest`, or the canonical `/generate_204` probe URL. When `canary_upstream` is unset, non-probe requests now return a plain text `404 Not Found`. When set, xp forwards method, path, query, non-hop-by-hop headers, status, response headers, and streaming bodies to that endpoint upstream. The outbound `Host` is normalized to the `canary_upstream` origin so localhost and name-based upstream services work predictably. Upstream mode is `auto`, `http1`, or explicit `h2c`; `auto` supports HTTP/1.1 and HTTPS ALPN HTTP/2.
- Admin UI 的默认 `New endpoint` VLESS 创建路径已收敛到同一托管合同：页面只提交 `port` 与可选 `canary_upstream` / `accepted_authorities`，服务端按节点 `access_host` 自动派生 `reality.dest=XP_VLESS_CANARY_BIND`、`server_names=[node.access_host]` 并写入 `managed_default=true`。legacy 非托管 VLESS 创建仅保留给显式 API 客户端兼容，不再是 UI 主路径。
- The reverse proxy is TLS-terminating HTTP reverse proxy behavior, not TCP passthrough and not a forward proxy. It supports streaming request/response bodies, SSE, large uploads/downloads, and WebSocket upgrade over an HTTP/1.1 upstream connection; explicit `h2c` is for non-upgrade HTTP traffic, and `CONNECT` is not part of the v1 contract.
- Ordinary HTTPS clients probing `https://<access_host[:vless_port]>/generate_204` receive the canary `204` through the VLESS ingress itself and never touch upstream.
- The endpoint detail page exposes a managed VLESS **Canary /generate_204** test for that ordinary HTTPS path. It fans out to every xp node, reports per-node status/latency/error, and is an immediate diagnostic for public ingress, TLS, REALITY fallback, and xp canary behavior; it is separate from the hourly cluster-wide proxy path probe and is not stored in endpoint probe history.
- Host-managed and container-managed nodes use the same managed-default endpoint contract. On host-managed nodes, `xp` startup and `xp-ops xp sync-node-meta` both reconcile the local default endpoint set; on container-managed nodes, `xp-ops container run` does the same after the local control plane is ready.
- Historical host-managed nodes with exactly one legacy VLESS endpoint on the node are auto-adopted into the managed-default contract during upgrade when that endpoint still predates the `managed_default` metadata flag; the runtime only rewrites that ingress to the loopback canary semantics after the canary itself is ready, and if canary preparation fails the old ingress stays untouched while `vless_https_canary_status.last_error` explains the blocker.
- This does not move the admin UI / cluster API onto the VLESS port.
- Mihomo relay groups prefer `https://<access_host[:managed_vless_port]>/generate_204`, then fall back to `api_base_url + /api/health`, then `https://www.gstatic.com/generate_204`.
- Legacy `XP_RELAY_PROBE_*` variables are removed; startup/sync now fails fast if they are still present.

Host-managed upgrade note:

- If `/etc/xp/xp.env` already declares `XP_DEFAULT_VLESS_PORT`, startup uses that port as the source of truth. `XP_DEFAULT_VLESS_SERVER_NAMES` is ignored for SNI selection after validation.
- If a historical host-managed node has no `XP_DEFAULT_VLESS_*` yet, but the node currently has exactly one legacy VLESS endpoint whose metadata still predates the `managed_default` flag, the new binary auto-adopts that endpoint on startup and rewrites `reality.dest` to the loopback canary only after the canary is healthy; when canary preparation is blocked, startup/sync leave the existing endpoint untouched and surface the error via `vless_https_canary_status`.
- If the node has multiple VLESS endpoints and none are already marked as managed-default, the runtime refuses to guess. In that case the operator must first decide which endpoint should be the managed default before expecting Mihomo relay probing to target that ingress.

Deployment note:

- `xp-ops deploy` now writes the managed-default endpoint contract into `/etc/xp/xp.env` when you pass `--default-vless-port` + `--default-vless-server-names` and/or `--default-ss-port`.
- `--vless-canary-acme-contact-email` is optional but recommended when you want the VLESS canary certificate flow to be fully operator-owned.
- The host-managed deploy path is therefore no longer container-only; the same one-shot flow now covers host-managed service nodes as well as official single-image container nodes.

Example host-managed bootstrap (systemd / RHEL-family included):

```bash
sudo -E xp-ops deploy \
  --node-name host-node-1 \
  --access-host edge-node-1.example.net \
  --account-id <cloudflare-account-id> \
  --hostname admin-node-1.example.com \
  --ddns \
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
- It also prepares DDNS runtime files and reconciles default managed SS/VLESS endpoints from container env on every start.
- For an existing joined node, set `XP_ADMIN_TOKEN_HASH` in the host-owned Compose environment
  file, then recreate the service to rotate the administrator credential. The entrypoint accepts
  only the low-memory Argon2id profile (`m=4096,t=3,p=1`) and atomically reconciles the persisted
  cluster hash before XP starts. A first join retains the leader-provided hash. Do not edit the
  running container or its data volume by hand.
- `xp` still reports `xray` health through `GET /api/health`.
- `cloudflared` is intentionally started outside `xp`'s built-in runtime supervisor, so the Web runtime pages treat `cloudflared` as disabled in container mode.

## `xp-ops mihomo redact` (subscription/config sanitization)

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
- The TUI covers the same host-managed managed-default inputs as `xp-ops deploy`, including `XP_DEFAULT_VLESS_*`, `XP_DEFAULT_SS_PORT`, and `XP_VLESS_CANARY_ACME_CONTACT_EMAIL`.

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
  - Maximum wait budget for the DNS-01 TXT to become visible on every authoritative nameserver before ACME validation starts.
- `XP_MESH_PROXY_URL` (default: unset)
  - Optional proxy URL for node-to-node control-plane traffic. With the `xp-ops init` static Xray config, use `socks5h://127.0.0.1:10808`.
  - This does not replace `XP_API_BASE_URL`; the public HTTPS origin remains the bootstrap and fallback path.

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
4. Excluded: `xp` admin port, Xray API, `mesh-proxy`, `cloudflared`, and outbound connections are not part of this panel.
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
  node_history_cache.json
  inbound_ip_usage.json
  service_runtime.json
  ddns_state.json
```

Notes:

- `cluster/` holds long-lived identity and TLS assets. Treat `cluster_ca_key.pem` as sensitive (private key).
- `raft/` holds the raft write-ahead log and snapshots.
- `state.json` and `usage.json` are raft-backed JSON snapshots; on schema mismatches, startup fails instead of silently migrating.
- `inbound_ip_usage.json` is a local-only high-frequency store for inbound IP presence (7-day retention, 1-minute bitmap window, Geo cache). It is **not** replicated via raft.
- `node_history_cache.json` stores local node history and Traffic analytics. Traffic sampling runs on UTC five-minute boundaries and writes the same Xray counter delta to node and real-user rollups. It retains at most 588 five-minute buckets (49 hours) and 90 UTC daily buckets; hourly rollups are not stored. Endpoint probe traffic is included in node totals but is never exposed as a normal user.
- Missing samples, first tracking, and counter resets remain partial and are surfaced as warnings; operators must not treat gaps as zero traffic. Deleting a user clears its stored history, deleting a node clears its node and user-node history, and removed memberships expire naturally with the retention windows.
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
3. Ensure `XP_DATA_DIR` exists and is writable by the service user.
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
- During static config rewrite, `xp-ops upgrade` preserves control-plane listener bindings that are already authoritative on the node: `XP_XRAY_API_ADDR` remains the source of truth for the `api` inbound, and an existing `mesh-proxy` inbound keeps its previous listener shape.
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
  returns `409 upgrade_already_running`.
- The Web UI polls status while the job is running. If `xp` restarts during the upgrade, the status
  file is used to recover the last known result.
- If a host one-shot runner fails before it can write a terminal status, the status endpoint
  reconciles the stale active status to `failed` instead of reporting `running` forever. On systemd
  nodes this reconciliation uses `xp-upgrade.service` failure state as the durable local fact.

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
  `xp` may run only `xp-openrc-upgrade-trigger --check` to verify the installed delegate, and may
  start only `/sbin/rc-service xp-upgrade start`. The helper accepts only `--check`, verifies the
  executable runner and exact start rule as root, and never starts the service. This lets the
  service detect a root-owned `0600` `/etc/doas.conf` without granting it read access.
  On an existing node that predates this helper, run `sudo xp-ops init` once after the ordinary
  `sudo xp-ops upgrade --version latest` has completed. The old running `xp-ops` cannot execute
  release code that it has not installed yet, so this root-approved reinitialization is required;
  readiness checks must not start the one-shot service as an implicit migration.
- Reference samples live at:
  - `docs/ops/systemd/xp-upgrade.service`
  - `docs/ops/systemd/xp-upgrade-trigger`
  - `docs/ops/systemd/sudoers-xp-upgrade`
  - `docs/ops/systemd/xp-upgrade.polkit.rules`
  - `docs/ops/openrc/xp-upgrade`
  - `docs/ops/openrc/xp-upgrade-trigger`
  - `docs/ops/openrc/doas-xp-upgrade.conf`

Rollback notes:

- The upgrade keeps a backup next to the install path as `<path>.bak.<unix-ts>`.
- On `xp` restart failures, `xp-ops upgrade` automatically rolls back to the previous `xp` binary.
- On managed-runtime or restart failures, `xp-ops upgrade` restores the previous Xray/cloudflared binaries, Xray config, and `xp` before returning failure.
- If that failure happened after a self-upgrade re-exec, `xp-ops upgrade` also restores the previous `xp-ops` binary instead of leaving the node on the newer operator binary.

### Deployment-specific upgrade paths

Use the path that matches the node shape instead of mixing procedures:

- Host-managed systemd/OpenRC nodes:
  - Upgrade binaries with `xp-ops upgrade` when the distro family is officially supported by `xp-ops`.
  - Alternatively, after `xp-ops init` has installed the one-shot runner and narrow privilege rule,
    start the same current-node upgrade from the Web UI.
  - Arch/Debian/Ubuntu/RHEL-family nodes are covered by the supported automation path.
  - If a host-managed node falls outside those distro families, upgrade the `xp` and `xp-ops` binaries manually, then restart `xp` and verify the post-upgrade checks below.
- Docker / Compose nodes:
  - Update the image tag or digest, then restart the container.
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
   - `--default-vless-port`
   - `--default-vless-server-names`
   - optional `--default-vless-fingerprint`
   - optional `--default-ss-port`
   - recommended `--vless-canary-acme-contact-email`
   - `--enable-services -y`
3. Confirm `/etc/xp/xp.env` now contains the managed-default endpoint keys and the canary/DDNS keys.
4. Restart validation:
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
   - `XP_DEFAULT_VLESS_PORT`
   - `XP_DEFAULT_VLESS_SERVER_NAMES`
   - optional `XP_DEFAULT_VLESS_FINGERPRINT`
   - optional `XP_DEFAULT_SS_PORT`
   - optional `XP_VLESS_CANARY_ACME_CONTACT_EMAIL`
3. Restart the container so `xp-ops container run` replays bootstrap/join, runtime reconcile, and default endpoint reconcile.
4. Validate:
   - container logs show successful `xp-ops container run`
   - `GET /api/admin/config` returns healthy `vless_https_canary_status`
   - `curl -Ik https://<access_host[:vless_port]>/generate_204` succeeds from outside the node path you actually use
   - Mihomo provider render uses `https://<access_host[:managed_vless_port]>/generate_204`
5. If the env intentionally removes `XP_DEFAULT_VLESS_*` or `XP_DEFAULT_SS_PORT`, expect the corresponding managed-default endpoint to be removed on next reconcile.

## Disaster recovery: quorum lost (single-node leader recovery)

Stable Raft membership treats every listed node as a voter. Long-lived learners, observer nodes,
`can_vote` flags, or `voter=false` configuration are not supported. A 2-voter topology is not an
acceptable production shape because losing either voter removes writable quorum; use at least 3
stable voters for production clusters.

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
- Treat `xp join` success as voter success: a node that cannot be promoted to voter must not be
  considered joined.
- Run `xp-ops xp sync-node-meta` on each node after updating `/etc/xp/xp.env` to ensure membership
  `NodeMeta` (leader discovery/forwarding) matches config.
- After all intended nodes are rejoined, confirm `/api/cluster/info` has a leader and endpoint
  creation succeeds through the admin UI/API. Any `membership.nodes - voter_ids` divergence is an
  incident and must be repaired by the leader-side guard or by explicit disaster recovery.

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
