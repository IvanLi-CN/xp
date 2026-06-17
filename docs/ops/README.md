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
- `XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID` is optional; when empty, `xp` derives the Cloudflare zone from `XP_ACCESS_HOST`.

Contract:

- `xp` terminates TLS for `GET/HEAD /generate_204` on the loopback canary and returns `204`.
- xp-managed/default VLESS/REALITY endpoints set `reality.dest` to that loopback canary, so ordinary HTTPS clients probing `https://<access_host[:vless_port]>/generate_204` receive the canary response through the VLESS ingress itself.
- Host-managed and container-managed nodes use the same managed-default endpoint contract. On host-managed nodes, `xp` startup and `xp-ops xp sync-node-meta` both reconcile the local default endpoint set; on container-managed nodes, `xp-ops container run` does the same after the local control plane is ready.
- Historical host-managed nodes with exactly one legacy VLESS endpoint on the node are auto-adopted into the managed-default contract during upgrade, but the runtime only rewrites that ingress to the loopback canary semantics after the canary itself is ready; if canary preparation fails, the old ingress stays untouched and `vless_https_canary_status.last_error` explains the blocker.
- This does not move the admin UI / cluster API onto the VLESS port.
- Mihomo relay groups prefer `https://<access_host[:managed_vless_port]>/generate_204`, then fall back to `api_base_url + /api/health`, then `https://www.gstatic.com/generate_204`.
- Legacy `XP_RELAY_PROBE_*` variables are removed; startup/sync now fails fast if they are still present.

Host-managed upgrade note:

- If `/etc/xp/xp.env` already declares `XP_DEFAULT_VLESS_*`, startup uses those values as the source of truth.
- If a historical host-managed node has no `XP_DEFAULT_VLESS_*` yet, but the node currently has exactly one legacy VLESS endpoint, the new binary auto-adopts that endpoint on startup and rewrites `reality.dest` to the loopback canary only after the canary is healthy; when canary preparation is blocked, startup/sync leave the existing endpoint untouched and surface the error via `vless_https_canary_status`.
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
  --default-vless-server-names 'public.sn.files.1drv.com,public.bn.files.1drv.com' \
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
  - Optional explicit Cloudflare zone id for DNS-01; when empty, `xp` derives the zone from `XP_ACCESS_HOST`.
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

1. Required: Xray static config enables `statsUserOnline=true` together with the existing traffic stats.
2. When `XP_IP_GEO_ENABLED=true`, nodes need outbound HTTPS access to `https://api.country.is/` so new public IPs can be resolved on first sight.
3. The node egress probe used by Mihomo region auto-grouping also relies on outbound HTTPS access to the public IP trace endpoint (default `https://cloudflare.com/cdn-cgi/trace`) and to `https://api.country.is/`.
4. `xp` caches resolved IP geo/operator fields inside `inbound_ip_usage.json`; API lookup failures only leave the affected fields empty and do not interrupt quota collection (the admin UI will show an `ip_geo_lookup_failed` warning after failed lookups).

Operational notes:

- No local Geo DB download/update job runs anymore, so `${XP_DATA_DIR}/geoip` is not used by the default IP usage pipeline.
- Upgrades from releases that used managed DB-IP geo enrichment must opt in again via `XP_IP_GEO_ENABLED=true`; otherwise `geo_source=missing` and geo fields stay empty.
- `statsUserOnline` is required for the online IP snapshot itself. If it is missing, `xp` keeps quota collection running and returns an `online_stats_unavailable` warning to the admin UI.
- `xp-ops init` now writes `/etc/xray/config.json` with `statsUserOnline=true` by default; nodes provisioned before this change should verify their static config before rollout.

Quick checks on a node:

```
jq '.policy.levels["0"]' /etc/xray/config.json
ls -l "${XP_DATA_DIR}/inbound_ip_usage.json" || true
jq '.online_stats_unavailable' "${XP_DATA_DIR}/inbound_ip_usage.json" 2>/dev/null || true
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
  inbound_ip_usage.json
  service_runtime.json
  ddns_state.json
```

Notes:

- `cluster/` holds long-lived identity and TLS assets. Treat `cluster_ca_key.pem` as sensitive (private key).
- `raft/` holds the raft write-ahead log and snapshots.
- `state.json` and `usage.json` are raft-backed JSON snapshots; on schema mismatches, startup fails instead of silently migrating.
- `inbound_ip_usage.json` is a local-only high-frequency store for inbound IP presence (7-day retention, 1-minute bitmap window, Geo cache). It is **not** replicated via raft.
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

## Upgrade and rollback strategy

### Recommended: upgrade via `xp-ops` (GitHub Releases)

`xp-ops` can upgrade both `xp` and `xp-ops` from GitHub Releases (Linux musl assets).

Upgrade both `xp` (installs to `/usr/local/bin/xp` and restarts the service) and `xp-ops`:

```
sudo xp-ops upgrade --version latest
```

Useful flags:

- `--dry-run` prints the resolved release + actions without downloading/writing/restarting.
- `--prerelease` (only with `--version latest`) selects the newest prerelease instead of stable.
- `--repo <owner/repo>` (or `XP_OPS_GITHUB_REPO=<owner/repo>`) overrides the default source repo.

UI notes:

- The Web UI header shows the current `xp` version (clickable) and can check whether a newer stable GitHub Release exists.
- The UI does not perform upgrades; upgrades are still expected to be done via `xp-ops upgrade`.
- If you override the upgrade source repo via `XP_OPS_GITHUB_REPO`, the version check uses the same repo.

Rollback notes:

- The upgrade keeps a backup next to the install path as `<path>.bak.<unix-ts>`.
- On upgrade failures, `xp-ops upgrade` automatically rolls back to the previous `xp` binary.

### Deployment-specific upgrade paths

Use the path that matches the node shape instead of mixing procedures:

- Host-managed systemd/OpenRC nodes:
  - Upgrade binaries with `xp-ops upgrade` when the distro family is officially supported by `xp-ops`.
  - Arch/Debian/Ubuntu/RHEL-family nodes are covered by the supported automation path.
  - If a host-managed node falls outside those distro families, upgrade the `xp` and `xp-ops` binaries manually, then restart `xp` and verify the post-upgrade checks below.
- Docker / Compose nodes:
  - Update the image tag or digest, then restart the container.
  - Let `xp-ops container run` perform runtime reconcile on startup.

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
5. If the node was an older single-VLESS deployment without `XP_DEFAULT_VLESS_*`, verify that startup auto-adopted the lone endpoint.
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

If a voter node is wiped and quorum is permanently lost (e.g. a 2-voter cluster loses 1 node),
the remaining node cannot elect a leader by itself. In this case you can force a single-node Raft
membership on the surviving node to restore write availability.

Warning:

- This is an unsafe recovery procedure. Any committed state that existed only on the wiped node is
  lost permanently.
- This rewrites local Raft persistence on disk. Stop `xp` before running it.

Procedure (surviving node):

1. Stop `xp` (systemd/OpenRC).
2. Run the recovery command:

```
sudo xp-ops xp recover-single-node -y
```

Notes:

- By default, `xp-ops` creates a backup copy at `${XP_DATA_DIR}/raft.bak-<timestamp>`. You can skip
  it with `--no-backup` (not recommended).
- After restart, leader election may take up to ~6-12 seconds (WAN-tuned defaults).

After recovery:

- Re-join the wiped node using a join token issued by the recovered leader (`/api/admin/cluster/join-tokens`),
  then run `xp join` on the wiped node and restart its service.
- Run `xp-ops xp sync-node-meta` on each node after updating `/etc/xp/xp.env` to ensure membership
  `NodeMeta` (leader discovery/forwarding) matches config.

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
