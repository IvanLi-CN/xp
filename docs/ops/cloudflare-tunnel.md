# Ops: Cloudflare Tunnel (public access)

This document describes how `xp-ops` provisions a Cloudflare Tunnel so you can reach `xp` from the public Internet without opening inbound ports.

It does **not** publish `XP_ACCESS_HOST` for user traffic. Tunnel-managed `hostname` and DDNS-managed `XP_ACCESS_HOST` are separate concerns:

- Tunnel: `XP_API_BASE_URL` / admin reachability (`hostname -> <tunnel-id>.cfargotunnel.com`, proxied CNAME)
- Runtime DDNS: `XP_ACCESS_HOST` / node endpoint reachability (`A` / `AAAA`, DNS only)

For the single-image Docker runtime, use the same Cloudflare API token and the same persisted files, but let `xp-ops container run` supervise the `cloudflared` process inside the container. When `XP_CLOUDFLARE_DDNS_ENABLED=true`, the same entrypoint also writes the runtime DDNS token file before starting `xp`. See `docs/ops/docker.md` for the Compose flow.

## What gets created / written

Local files (on the target server, root-managed):

- `/etc/xp-ops/cloudflare_tunnel/settings.json` (non-secret)
- `/etc/xp-ops/cloudflare_tunnel/api_token` (secret, `0600`)
- `/etc/cloudflared/<tunnel-id>.json` (secret, `0600`)
- `/etc/cloudflared/config.yml` (non-secret)

Cloudflare-side resources:

- A Tunnel (under the given `account_id`)
- A Tunnel configuration (ingress)
- A DNS record (CNAME, proxied) for `hostname` → `<tunnel-id>.cfargotunnel.com`
- Optional runtime-managed `A` / `AAAA` records for `XP_ACCESS_HOST` when deploy is run with `--ddns`

## Shared Tunnel safety

`xp-ops` supports an existing, shared Tunnel and keeps one `cloudflared` process.
It owns only the requested XP hostname:

- Local `config.yml` is edited in place. Unrelated keys, comments, quoting, order, and formatting
  remain intact. If the file did not contain local `ingress`, XP does not add one.
- The current remote Tunnel configuration is read before changes. Rules for other hostnames,
  protocol-specific services, `originRequest`, unknown top-level fields, and a valid final catch-all
  remain unchanged. All rules for the requested XP hostname are replaced with the current XP origin.
- A valid existing final catch-all is retained. A missing catch-all is completed with
  `http_status:404`; multiple or non-final catch-all rules are rejected without writes.
- Existing CNAME records are adopted only when they exactly point to the configured XP Tunnel.
  DNS PATCH changes only the owned CNAME content, leaving TTL, proxied mode, comments, and other
  record attributes untouched.

Before replacing a changed local configuration that contains local `ingress`, `xp-ops` runs
`cloudflared tunnel ingress validate` against a candidate file and atomically installs it. A
remote-Tunnel configuration without local `ingress` skips that inapplicable local validation.
A host-managed node then restarts the existing service; it never starts a second `cloudflared`
program. If a later write or restart fails, the command attempts reverse-order rollback of remote
ingress, DNS, local files, settings, credentials, and the service configuration. An incomplete
rollback reports the retained local snapshot paths for manual recovery.

Cloudflare can return `1055: Configuration for tunnel not found` after XP creates a new
remote-config Tunnel because it has no configuration yet. `xp-ops` treats that exact fresh-Tunnel
response as an empty remote configuration, then writes the owned ingress. A read error for an
existing Tunnel, or any other error, remains terminal and follows the normal rollback path.

When a rerun finds a same-named Tunnel that matches the persisted XP account, zone, hostname,
Tunnel ID, and local credentials whose `TunnelID` matches, `xp-ops deploy --non-interactive -y`
reuses it directly. It does not generate a suffixed Tunnel name for that verified local deployment
state.

### Moving an existing XP Tunnel

Changing the persisted Tunnel ID automatically migrates the persisted XP hostname after confirming
that the old settings hostname, zone, DNS CNAME, and credentials all belong to the same XP
installation. After a successful single-host preflight, XP migrates only its persisted hostname:

```bash
sudo xp-ops cloudflare provision \
  --account-id <id> --zone-id <id> --hostname app.example.com \
  --origin-url http://127.0.0.1:62416 \
  --tunnel-id <target-tunnel-id>
```

Any incomplete ownership proof fails before a write; XP does not guess across zones, accounts, or
DNS records. A single cloudflared process can migrate automatically only when the legacy Tunnel
contains the persisted XP hostname alone. A shared legacy Tunnel is rejected before writes because
its other hostnames would otherwise lose their connector.

Use `--dry-run` to perform read-only preflight and receive an ingress/DNS impact summary.
It never writes a file, calls a mutating Cloudflare API, or restarts a service.

## Required API token permissions

Create a Cloudflare API token with:

- Account: `Cloudflare Tunnel:Edit`
- Zone: `DNS:Edit`

`xp-ops` reads the token from:

- `--cloudflare-token <token>` / `--cloudflare-token-stdin` (for one-shot deploy; see below), or
- `CLOUDFLARE_API_TOKEN` (recommended for CI / one-off runs), or
- `/etc/xp-ops/cloudflare_tunnel/api_token`

The token is never printed to stdout/stderr by design.

When DDNS is enabled, `xp-ops deploy --ddns` also writes an `xp`-readable copy to:

- `/etc/xp/cloudflare_ddns_api_token` (secret, `0640`, typically `root:xp`)

`xp` uses that runtime token file together with `XP_CLOUDFLARE_DDNS_*` settings to reconcile `XP_ACCESS_HOST`.
The managed VLESS HTTPS canary DNS-01 flow also reuses the same xp-readable runtime token file by default (`XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE=/etc/xp/cloudflare_ddns_api_token`).

## Typical workflow

1. Save token (optional):

```
export CLOUDFLARE_API_TOKEN=...
sudo -E xp-ops cloudflare token set --from-env CLOUDFLARE_API_TOKEN
```

2. Provision tunnel + DNS + local runtime files:

```
sudo xp-ops cloudflare provision \
  --tunnel-name xp-node-1 \
  --account-id <id> \
  --zone-id <id> \
  --hostname app.example.com \
  --origin-url http://127.0.0.1:62416
```

If you are using the recommended one-shot deploy flow, `xp-ops deploy` can infer missing values:

```
sudo -E xp-ops deploy \
  --node-name node-1 \
  --access-host node-1.example.net \
  --account-id <id> \
  --hostname node-1.example.com \
  -y
```

To enable runtime DDNS for `XP_ACCESS_HOST` on the same node:

```
sudo -E xp-ops deploy \
  --node-name node-1 \
  --access-host node-1.example.net \
  --ddns \
  --hostname node-1.example.com \
  --account-id <id> \
  -y
```

Notes:

- `--ddns` may be used with or without `--cloudflare`.
- `--ddns-zone-id` is optional; when omitted, deploy tries to derive the Cloudflare zone from `--access-host`.
- Runtime DDNS keeps records normalized as `DNS only` + `TTL=Auto`.
- If Cloudflare already has exactly one `A` / `AAAA` for `XP_ACCESS_HOST`, `xp` adopts and updates it. Multiple same-type records are treated as an operator error and are not modified automatically.
- For managed-default VLESS/SS bootstrap on host-managed nodes, pass `--default-vless-port`,
  `--default-vless-server-names`, and optionally `--default-vless-fingerprint` /
  `--default-ss-port`; `xp-ops deploy` writes those into `/etc/xp/xp.env`. These values bootstrap
  missing endpoints only. Existing or auto-adopted ports remain cluster-owned and are changed
  through the Admin UI/API.
- A Cloudflare-enabled host-managed deploy writes `XP_ENABLE_CLOUDFLARE=true` and its validated
  account ID, zone ID, and hostname into `/etc/xp/xp.env`. A recovery deploy converges those
  deployment-owned values from the validated plan rather than requiring a manual env edit.
- `--vless-canary-acme-contact-email` is the operator-controlled ACME contact for the loopback HTTPS canary and should be set on the same one-shot deploy if you want a fully reproducible certificate flow.
- `--ip-geo` explicitly writes `XP_IP_GEO_ENABLED=true` for inbound Geo enrichment using the
  default `https://api.country.is` origin. Omitting it preserves an existing value and keeps new
  nodes on the program default.
- For host-managed join deployments, Tunnel/DNS provisioning stages the configuration only.
  Deploy joins and writes `/etc/xp/xp.env` before enabling then starting or restarting `xray`, `xp`, and `cloudflared`;
  public `api_base_url/health` must then return HTTP `200`. A `post_join_health_failed` result
  keeps the member and local metadata for a retry after repairing the service or public route.

- If you want to provide the Cloudflare token from the command line (not recommended, can leak via shell history / `ps`):

```
sudo xp-ops deploy \
  --node-name node-1 \
  --access-host node-1.example.net \
  --cloudflare \
  --account-id <id> \
  --hostname node-1.example.com \
  --cloudflare-token <token> \
  -y
```

- To reduce leakage risk, prefer stdin:

```
printf "%s" "<token>" | sudo xp-ops deploy \
  --node-name node-1 \
  --access-host node-1.example.net \
  --cloudflare \
  --account-id <id> \
  --hostname node-1.example.com \
  --cloudflare-token-stdin \
  -y
```

- `--zone-id` is optional for `deploy`: it will be resolved from `hostname` (or `access-host` if hostname is not provided).
- `--hostname` is optional for `deploy` if `zone-id` is provided; the hostname will be derived as `<node-name>.<zone>`.
- `--xp-bin` is optional for `deploy`: omit it if `xp` is already installed at `/usr/local/bin/xp`.
- `-y` enables interactive preflight confirmation and hostname conflict resolution.

3. Verify services:

```
sudo xp-ops status
sudo systemctl status cloudflared.service
```

## Troubleshooting checklist

- Token missing: ensure `CLOUDFLARE_API_TOKEN` is set or `/etc/xp-ops/cloudflare_tunnel/api_token` exists.
- Cloudflare API errors: verify token scopes and `account_id/zone_id`.
- DNS issues: verify the record in `settings.json` and in Cloudflare Dashboard.
- Local runtime:
  - `/etc/cloudflared/config.yml` exists and references the correct `credentials-file`.
  - `/etc/cloudflared/<tunnel-id>.json` exists and is `0600`.

DDNS-specific checks:

- `/etc/xp/cloudflare_ddns_api_token` exists, is readable by the `xp` service user, and is not empty.
- `XP_CLOUDFLARE_DDNS_ENABLED=true` is present in `/etc/xp/xp.env`.
- `XP_ACCESS_HOST` is a valid FQDN under the expected Cloudflare zone.
- `/api/admin/nodes/<node>/runtime` shows a `ddns` component; `degraded` / `down` states will include the last error.
- `${XP_DATA_DIR}/ddns_state.json` reflects the last synced IPv4 / IPv6 and any pending fast-mode window.

## Security notes

Cloudflare Tunnel publishes your hostname on the public Internet. Strongly consider protecting the hostname with:

- a Cloudflare Access policy, and/or
- strict authentication on `xp` (ensure `XP_ADMIN_TOKEN` is set and not empty).
