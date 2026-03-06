# HTTP APIs

## GET `/api/admin/users/{user_id}/subscription-mihomo-profile`

Response `200`:

```json
{
  "template_yaml": "string",
  "extra_proxies_yaml": "string",
  "extra_proxy_providers_yaml": "string"
}
```

- 若用户存在但未配置，返回空字符串字段（不是 404）。

## PUT `/api/admin/users/{user_id}/subscription-mihomo-profile`

Request body:

```json
{
  "template_yaml": "string",
  "extra_proxies_yaml": "string",
  "extra_proxy_providers_yaml": "string"
}
```

Validation:

- `template_yaml` 必填，且 YAML 根必须为 mapping。
- `extra_proxies_yaml` 允许空字符串；非空时 YAML 根必须为 sequence。
- `extra_proxy_providers_yaml` 允许空字符串；非空时 YAML 根必须为 mapping。

Normalization:

- 若 `template_yaml` 顶层包含 `proxies`（sequence）或 `proxy-providers`（mapping），服务端会自动抽取到
  `extra_proxies_yaml` / `extra_proxy_providers_yaml` 并从 `template_yaml` 移除；响应会返回规范化后的结果
  （YAML 注释/anchors 不保证保留）。

Response `200`: 同 GET 结构。

Errors:

- `404 not_found`: user 不存在
- `400 invalid_request`: YAML 解析失败或根类型不匹配

## GET `/api/sub/{subscription_token}?format=mihomo`

- 当用户已配置 Mihomo profile：
  - 使用 profile.template_yaml 为基底渲染；
  - 系统重建并覆盖 `proxies`、`proxy-providers`；
  - relay 组 `🛣️ Japan|HongKong|Korea` 的 `use` 自动注入所有 provider 名称；
  - 系统会覆盖并注入一组“动态相关”的 `proxy-groups`（地区入口组、落地组与落地池），使 mixin config 不需要写死
    providers 列表或主力节点名称。
- 当用户未配置 Mihomo profile：回退到 clash 输出。

Response:

- `200 text/yaml; charset=utf-8`
