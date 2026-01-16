# HTTP APIs Contracts（#0013）

本文件用于冻结运维 CLI 对接 Cloudflare Tunnel 所需的外部 HTTP API 形状（仅作对接契约，不包含实现细节）。

## External Cloudflare REST APIs (depended) (Change: None)

说明：此处只冻结实现需要对接的最小外部 API 形状，便于在实现阶段将 Cloudflare 调用封装并编写可替换的 client/mock。

Base:

- Host: `api.cloudflare.com`
- Auth: `Authorization: Bearer <CLOUDFLARE_API_TOKEN>`
- Content-Type: `application/json`
- Token permissions (normative):
  - Account: `Cloudflare Tunnel:Edit`（用于 create tunnel / put tunnel configuration）
  - Zone: `DNS:Edit`（用于 create/patch DNS record）

### Create tunnel

- Method: `POST`
- Path: `/client/v4/accounts/{account_id}/cfd_tunnel`
- Request:
  - Body: `{ name: string; config_src?: "cloudflare" }`
- Response (success): includes
  - `result.id`（tunnel_id, UUID）
  - `result.credentials_file`（用于落盘为 `/etc/cloudflared/<tunnel-id>.json`）
  - `result.token`（可选；实现阶段不应把 token 放在进程参数中）

### Put tunnel configuration (ingress)

- Method: `PUT`
- Path: `/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations`
- Request (minimal):
  - Body:
    - `config.ingress[]`: first rule uses `{ hostname, service: origin_url }`（不使用 `path` 字段）
    - final catch-all uses `{ service: "http_status:404" }`

### Create DNS record for public hostname

- Method: `POST`
- Path: `/client/v4/zones/{zone_id}/dns_records`
- Request (minimal):
  - Body:
    - `type: "CNAME"`
    - `name: hostname`
    - `content: "${tunnel_id}.cfargotunnel.com"`
    - `proxied: true`

### Update DNS record (idempotent rerun)

- Method: `PATCH`
- Path: `/client/v4/zones/{zone_id}/dns_records/{dns_record_id}`
- Request (minimal):
  - Body: same as “Create DNS record”（at least `content` + `proxied`）
