# HTTP APIs

## GET `/api/admin/config`

Response `200`:

```json
{
  "bind": "string",
  "xray_api_addr": "string",
  "data_dir": "string",
  "node_name": "string",
  "access_host": "string",
  "api_base_url": "string",
  "quota_poll_interval_secs": 10,
  "quota_auto_unban": true,
  "ip_geo_enabled": false,
  "ip_geo_origin": "string",
  "admin_token_present": true,
  "admin_token_masked": "********",
  "mihomo_delivery_mode": "legacy"
}
```

Notes:

- `mihomo_delivery_mode` 为持久化全局设置，只允许 `legacy|provider`。
- 其它字段仍保持只读安全视图。

## PATCH `/api/admin/config`

Request body:

```json
{
  "mihomo_delivery_mode": "provider"
}
```

Validation:

- 仅接受 `mihomo_delivery_mode`；值必须是 `legacy` 或 `provider`。

Response `200`: 与 `GET /api/admin/config` 相同结构。

Errors:

- `400 invalid_request`: 请求体缺失或值非法
- `401 unauthorized`: 缺失/错误 admin token

## GET `/api/sub/{subscription_token}?format=mihomo`

- canonical Mihomo URL。
- 当用户未配置 Mihomo profile 时，仍回退到 clash 输出。
- 当用户已配置 Mihomo profile 时：
  - `mihomo_delivery_mode=legacy` => 返回现有 legacy Mihomo 主配置；
  - `mihomo_delivery_mode=provider` => 返回 provider 主配置。

Response:

- `200 text/yaml; charset=utf-8`

## GET `/api/sub/{subscription_token}/mihomo/legacy`

- 始终返回 legacy Mihomo 主配置。
- 当用户未配置 Mihomo profile 时，回退到 clash 输出。

Response:

- `200 text/yaml; charset=utf-8`

## GET `/api/sub/{subscription_token}/mihomo/provider`

- 始终返回 provider 方案的 Mihomo 主配置。
- 当用户未配置 Mihomo profile 时，回退到 clash 输出。
- 顶层 `proxy-providers` 必含系统 provider `xp-system-generated` 与用户 `extra_proxy_providers_yaml`。
- 顶层 `proxies` 保留 `extra_proxies_yaml` 与系统 `{base}-reality` / `{base}-chain`。
- 系统 provider 的 `url` 必须指向当前请求对外 origin 下的 `/api/sub/{token}/mihomo/provider/system`。

Response:

- `200 text/yaml; charset=utf-8`

## GET `/api/sub/{subscription_token}/mihomo/provider/system`

- 返回系统 provider payload，根为：

```yaml
proxies:
  - name: tokyo-ss
    type: ss
    # ...
```

Rules:

- 仅包含系统隐藏直连节点（当前为 `-ss`）；不包含 `{base}-reality` 与 `{base}-chain`。
- provider payload 可被 Mihomo `proxy-providers.type=http` 直接消费。
- 不依赖用户是否配置 Mihomo profile；即使主配置路径因缺少 profile 回退 clash，system payload 路径仍可单独返回系统隐藏直连节点。

Response:

- `200 text/yaml; charset=utf-8`

Errors:

- `404 not_found`: token 不存在
- `400 invalid_request`: provider 保留名冲突或其它用户配置不可恢复错误
