# Ops: single-image Docker deployment

This guide covers the official single-image runtime for one `xp` cluster node per container.

Owner-facing deployment walkthrough:

- `docs/ops/docker-deployment-guide.md`

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

| Key                                            | Required when                | Notes                                                                                                                                   |
| ---------------------------------------------- | ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `XP_NODE_NAME`                                 | always                       | Node display name                                                                                                                       |
| `XP_ADMIN_TOKEN` or `XP_ADMIN_TOKEN_HASH`      | bootstrap node               | Required on every bootstrap-node start                                                                                                  |
| `XP_JOIN_TOKEN`                                | join node first start        | Safe to keep set after join; restart will not re-run `xp join` if data already exists                                                   |
| `XP_API_BASE_URL`                              | `XP_ENABLE_CLOUDFLARE=false` | Must be an HTTPS origin                                                                                                                 |
| `XP_ENABLE_CLOUDFLARE=true`                    | optional                     | Enables Cloudflare provisioning + local `cloudflared`                                                                                   |
| `XP_CLOUDFLARE_ACCOUNT_ID`                     | tunnel enabled               | Cloudflare account id                                                                                                                   |
| `XP_CLOUDFLARE_HOSTNAME`                       | tunnel enabled               | Public hostname served by Tunnel                                                                                                        |
| `XP_CLOUDFLARE_ZONE_ID`                        | optional                     | Strongly recommended to avoid a zone lookup on startup                                                                                  |
| `XP_CLOUDFLARE_TUNNEL_NAME`                    | optional                     | Defaults to `xp-<node-name>`                                                                                                            |
| `XP_ACCESS_HOST`                               | optional                     | Recommended when DDNS is enabled; use the public endpoint hostname (for example `node-1-ep.example.com`)                                |
| `XP_CLOUDFLARE_DDNS_ENABLED=true`              | optional                     | Enables runtime DDNS for `XP_ACCESS_HOST`                                                                                               |
| `XP_CLOUDFLARE_DDNS_ZONE_ID`                   | DDNS enabled                 | Optional when Tunnel and DDNS share the same zone; otherwise provide it explicitly                                                      |
| `XP_VLESS_CANARY_BIND`                         | optional                     | Loopback TLS canary bind for xp-managed VLESS fallback; defaults to `127.0.0.1:39043`                                                   |
| `XP_VLESS_CANARY_ACME_CONTACT_EMAIL`           | optional                     | ACME contact email for Let's Encrypt DNS-01                                                                                             |
| `XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID`           | optional                     | Overrides the canary DNS-01 zone; when unset, runtime first reuses `XP_CLOUDFLARE_DDNS_ZONE_ID`, then falls back to host-derived lookup |
| `XP_VLESS_CANARY_DNS_PROPAGATION_TIMEOUT_SECS` | optional                     | DNS-01 authoritative visibility wait budget; defaults to `180` seconds                                                                  |
| `XP_MESH_PROXY_URL`                            | optional                     | Enables xp-to-xp control-plane requests through the local proxy; use `socks5h://127.0.0.1:10808` with the bundled Xray config           |
| `XP_DEFAULT_VLESS_PORT`                        | optional                     | Enables the managed default VLESS endpoint; managed SNI is derived from `XP_ACCESS_HOST`                                                |
| `XP_DEFAULT_VLESS_SERVER_NAMES`                | optional                     | Deprecated compatibility input; values are validated when present but no longer choose managed VLESS SNI                                |
| `XP_DEFAULT_VLESS_FINGERPRINT`                 | optional                     | Defaults to `chrome`                                                                                                                    |
| `XP_DEFAULT_SS_PORT`                           | optional                     | Enables managed default SS2022 endpoint                                                                                                 |
| `CLOUDFLARE_API_TOKEN`                         | tunnel enabled               | Required on every start when Tunnel is enabled                                                                                          |

### Derived values

- `XP_BIND` defaults to `0.0.0.0:62416`
- `XP_XRAY_API_ADDR` defaults to `127.0.0.1:10085`
- `XP_DATA_DIR` defaults to `/var/lib/xp/data`
- When `XP_ENABLE_CLOUDFLARE=true` and `XP_API_BASE_URL` is unset, it becomes `https://<XP_CLOUDFLARE_HOSTNAME>`
- When `XP_ACCESS_HOST` is unset, it is derived from `XP_CLOUDFLARE_HOSTNAME` or `XP_API_BASE_URL`
- `XP_VLESS_CANARY_BIND` defaults to `127.0.0.1:39043`
- When `XP_CLOUDFLARE_DDNS_ENABLED=true`, `xp-ops container run` writes the runtime DDNS token file before starting `xp` and injects the resolved `XP_CLOUDFLARE_DDNS_ZONE_ID`
- The bundled static Xray config exposes a loopback-only SOCKS listener at `127.0.0.1:10808` for optional control-plane relay. It is disabled unless `XP_MESH_PROXY_URL` is set.

### Persistent volumes

Mount all three of these paths:

- `/var/lib/xp/data`
- `/etc/cloudflared`
- `/etc/xp-ops/cloudflare_tunnel`

They persist:

- cluster metadata / raft state / certificates
- VLESS HTTPS canary ACME account key / cert / key under `/var/lib/xp/data/vless-https-canary`
- `cloudflared` credentials and config
- Cloudflare Tunnel settings (`settings.json`)

## Automatic reconciliation on restart

The container entrypoint treats the mounted data volume as the source of truth for `cluster_id` / `node_id`, but it also reconciles operator-managed fields from environment variables:

- Existing `metadata.json` is automatically realigned when `XP_NODE_NAME`, `XP_ACCESS_HOST`, or `XP_API_BASE_URL` changes
- After `xp` is running, the same values are synced back into the Raft state machine and membership metadata
- On join nodes, the first runtime reconcile reuses the `leader_api_base_url` carried by `XP_JOIN_TOKEN`, so default endpoints and node-meta sync do not depend on the local follower learning leader routing first
- Managed default endpoints are reconciled from env on every start:
  - first start creates them if missing
  - later env changes patch the existing managed endpoints in place
  - removing the env stops managing that endpoint and deletes the managed one if it was previously created/adopted by the container entrypoint

The managed default endpoint contract is:

- VLESS: set `XP_DEFAULT_VLESS_PORT`; managed SNI is derived from `XP_ACCESS_HOST`
- SS2022: set `XP_DEFAULT_SS_PORT`

For managed VLESS REALITY endpoints, `server_names` is fixed to `[XP_ACCESS_HOST]` without a port and `reality.dest` is automatically set to `XP_VLESS_CANARY_BIND`. `XP_DEFAULT_VLESS_SERVER_NAMES` is accepted only as a deprecated compatibility input and does not choose managed SNI. Each managed VLESS endpoint may carry its own `canary_upstream` plus an `accepted_authorities` alias set; `GET/HEAD /generate_204` is always answered by xp, and other requests route by canonical `Host`/`:authority` or one of the accepted aliases to the endpoint upstream. Public misses are exposed as plain text `404 Not Found`.

If the entrypoint needs to take over an existing endpoint and there is exactly one endpoint of that kind on the current node, it adopts that endpoint instead of creating a duplicate. Multiple same-kind endpoints are treated as an operator error and must be cleaned up manually.

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
export XP_ACCESS_HOST=node-1-ep.example.com
export XP_CLOUDFLARE_DDNS_ENABLED=true
export XP_CLOUDFLARE_DDNS_ZONE_ID=...
export XP_VLESS_CANARY_ACME_CONTACT_EMAIL=ops@example.com
export XP_DEFAULT_VLESS_PORT=53842
export XP_DEFAULT_SS_PORT=53843
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
export XP_ACCESS_HOST=node-2-ep.example.com
export XP_CLOUDFLARE_DDNS_ENABLED=true
export XP_CLOUDFLARE_DDNS_ZONE_ID=...
export XP_VLESS_CANARY_ACME_CONTACT_EMAIL=ops@example.com
export XP_DEFAULT_VLESS_PORT=53842
export XP_DEFAULT_SS_PORT=53843
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
- it also prepares the DDNS runtime token file when `XP_CLOUDFLARE_DDNS_ENABLED=true`
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
- Container restarts with unexpected node identity: inspect logs for the automatic node-meta realignment and verify `XP_NODE_NAME`, `XP_ACCESS_HOST`, and `XP_API_BASE_URL`
- DDNS is enabled but `XP_ACCESS_HOST` does not update: confirm `XP_CLOUDFLARE_DDNS_ENABLED=true`, `XP_CLOUDFLARE_DDNS_ZONE_ID`, and `CLOUDFLARE_API_TOKEN`
- Managed VLESS HTTPS canary fails on `https://<access_host[:vless_port]>/generate_204`: confirm `GET /api/admin/config` shows a healthy `vless_https_canary_status`, the managed VLESS endpoint exists on that host, and `XP_DEFAULT_VLESS_PORT` matches the probed ingress port
- Default endpoint reconcile fails: ensure only one VLESS / one SS2022 endpoint exists on the node before asking the container to adopt them
- Healthcheck fails but container is up: inspect `docker logs` for `xray` or `xp` child-process exits
