# Ops: single-image Docker deployment

This guide covers the official single-image runtime for one `xp` cluster node per container.

## What the image contains

`ghcr.io/ivanli-cn/xp` bundles:

- `xp`
- `xp-ops`
- real embedded `web/dist`
- `xray`
- `cloudflared`
- `tini`

The image entrypoint is fixed to:

```bash
xp-ops container run
```

## Runtime contract

### Required environment variables

| Key                                       | Required when                | Notes                                                                                 |
| ----------------------------------------- | ---------------------------- | ------------------------------------------------------------------------------------- |
| `XP_NODE_NAME`                            | always                       | Node display name                                                                     |
| `XP_ADMIN_TOKEN` or `XP_ADMIN_TOKEN_HASH` | bootstrap node               | Required on every bootstrap-node start                                                |
| `XP_JOIN_TOKEN`                           | join node first start        | Safe to keep set after join; restart will not re-run `xp join` if data already exists |
| `XP_API_BASE_URL`                         | `XP_ENABLE_CLOUDFLARE=false` | Must be an HTTPS origin                                                               |
| `XP_ENABLE_CLOUDFLARE=true`               | optional                     | Enables Cloudflare provisioning + local `cloudflared`                                 |
| `XP_CLOUDFLARE_ACCOUNT_ID`                | tunnel enabled               | Cloudflare account id                                                                 |
| `XP_CLOUDFLARE_HOSTNAME`                  | tunnel enabled               | Public hostname served by Tunnel                                                      |
| `XP_CLOUDFLARE_ZONE_ID`                   | optional                     | Strongly recommended to avoid a zone lookup on startup                                |
| `XP_CLOUDFLARE_TUNNEL_NAME`               | optional                     | Defaults to `xp-<node-name>`                                                          |
| `CLOUDFLARE_API_TOKEN`                    | tunnel enabled               | Required on every start when Tunnel is enabled                                        |

### Derived values

- `XP_BIND` defaults to `0.0.0.0:62416`
- `XP_XRAY_API_ADDR` defaults to `127.0.0.1:10085`
- `XP_DATA_DIR` defaults to `/var/lib/xp/data`
- When `XP_ENABLE_CLOUDFLARE=true` and `XP_API_BASE_URL` is unset, it becomes `https://<XP_CLOUDFLARE_HOSTNAME>`
- When `XP_ACCESS_HOST` is unset, it is derived from `XP_CLOUDFLARE_HOSTNAME` or `XP_API_BASE_URL`

### Persistent volumes

Mount all three of these paths:

- `/var/lib/xp/data`
- `/etc/cloudflared`
- `/etc/xp-ops/cloudflare_tunnel`

They persist:

- cluster metadata / raft state / certificates
- `cloudflared` credentials and config
- Cloudflare Tunnel settings (`settings.json`)

## Bootstrap node

Reference Compose file:

- `deploy/docker/compose.bootstrap.yml`

Typical flow:

```bash
export XP_IMAGE=ghcr.io/ivanli-cn/xp:latest
export XP_NODE_NAME=node-1
export XP_ADMIN_TOKEN='replace-with-a-strong-secret'
export XP_ENABLE_CLOUDFLARE=true
export XP_CLOUDFLARE_ACCOUNT_ID=...
export XP_CLOUDFLARE_ZONE_ID=...
export XP_CLOUDFLARE_HOSTNAME=node-1.example.com
export CLOUDFLARE_API_TOKEN=...

docker compose -f deploy/docker/compose.bootstrap.yml up -d
```

Local checks:

```bash
curl -fsS http://127.0.0.1:62416/api/health
curl -fsS http://127.0.0.1:62416/
```

Notes:

- The compose example publishes `127.0.0.1:${XP_HOST_PORT:-62416}:62416`; change `XP_HOST_PORT` if you run multiple nodes on one host.
- If you disable Tunnel, set `XP_ENABLE_CLOUDFLARE=false` and provide `XP_API_BASE_URL=https://<public-origin>`.

## Join node

Reference Compose file:

- `deploy/docker/compose.join.yml`

Typical flow:

```bash
export XP_IMAGE=ghcr.io/ivanli-cn/xp:latest
export XP_NODE_NAME=node-2
export XP_JOIN_TOKEN='replace-with-a-real-join-token'
export XP_ENABLE_CLOUDFLARE=true
export XP_CLOUDFLARE_ACCOUNT_ID=...
export XP_CLOUDFLARE_ZONE_ID=...
export XP_CLOUDFLARE_HOSTNAME=node-2.example.com
export CLOUDFLARE_API_TOKEN=...

docker compose -f deploy/docker/compose.join.yml up -d
```

Join sequencing:

1. `xp-ops container run` provisions or reuses Tunnel state
2. it starts `cloudflared`
3. it waits for `https://<hostname>/health` to stop returning Cloudflare `530`
4. it executes `xp join --token ...`
5. it launches `xray` and long-running `xp`

## Cloudflare behavior in container mode

Container mode reuses the same provisioning APIs and file layout as the host-managed flow, but the process model is different:

- `xp-ops container run` owns the `cloudflared` child process
- `xp` is started with `--cloudflared-restart-mode none`
- the Web runtime pages therefore treat `cloudflared` as disabled in container mode
- inspect container logs / orchestrator status for `cloudflared` lifecycle debugging

The persisted Tunnel state prevents duplicate Tunnel / DNS creation on restart as long as these volumes stay attached:

- `/etc/cloudflared`
- `/etc/xp-ops/cloudflare_tunnel`

## Image publishing

Stable releases publish:

- `ghcr.io/ivanli-cn/xp:vX.Y.Z`
- `ghcr.io/ivanli-cn/xp:X.Y.Z`
- `ghcr.io/ivanli-cn/xp:latest`

Pre-releases publish:

- `ghcr.io/ivanli-cn/xp:vX.Y.Z-...`
- `ghcr.io/ivanli-cn/xp:X.Y.Z-...`

## Troubleshooting checklist

- Missing bootstrap token/hash: confirm `XP_ADMIN_TOKEN` or `XP_ADMIN_TOKEN_HASH`
- Missing join token: confirm `XP_JOIN_TOKEN`
- Tunnel enabled but startup fails before join: confirm `CLOUDFLARE_API_TOKEN`, account id, hostname, and zone id
- Container restarts with metadata mismatch: confirm `XP_NODE_NAME`, `XP_ACCESS_HOST`, and `XP_API_BASE_URL` still match the existing data volume
- Healthcheck fails but container is up: inspect `docker logs` for `xray` or `xp` child-process exits
