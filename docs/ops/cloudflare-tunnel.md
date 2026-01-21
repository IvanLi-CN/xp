# Ops: Cloudflare Tunnel (public access)

This document describes how `xp-ops` provisions a Cloudflare Tunnel so you can reach `xp` from the public Internet without opening inbound ports.

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

## Required API token permissions

Create a Cloudflare API token with:

- Account: `Cloudflare Tunnel:Edit`
- Zone: `DNS:Edit`

`xp-ops` reads the token from:

- `CLOUDFLARE_API_TOKEN` (recommended for CI / one-off runs), or
- `/etc/xp-ops/cloudflare_tunnel/api_token`

The token is never printed to stdout/stderr by design.

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

## Security notes

Cloudflare Tunnel publishes your hostname on the public Internet. Strongly consider protecting the hostname with:

- a Cloudflare Access policy, and/or
- strict authentication on `xp` (ensure `XP_ADMIN_TOKEN` is set and not empty).
