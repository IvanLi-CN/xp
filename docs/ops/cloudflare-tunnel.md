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
