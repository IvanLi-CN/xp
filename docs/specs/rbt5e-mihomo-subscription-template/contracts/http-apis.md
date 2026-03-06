# HTTP APIs

## GET `/api/admin/users/{user_id}/subscription-mihomo-profile`

Response `200`:

```json
{
  "mixin_yaml": "string",
  "extra_proxies_yaml": "string",
  "extra_proxy_providers_yaml": "string"
}
```

- 若用户存在但未配置，返回空字符串字段（不是 404）。
- 响应只返回 `mixin_yaml`；不再返回旧字段 `template_yaml`。

## PUT `/api/admin/users/{user_id}/subscription-mihomo-profile`

Request body:

```json
{
  "mixin_yaml": "string",
  "extra_proxies_yaml": "string",
  "extra_proxy_providers_yaml": "string"
}
```

Backward compatibility:

- 请求体兼容旧字段 `template_yaml` 一轮实现周期；若只传旧字段，服务端会按 `mixin_yaml` 语义处理。

Validation:

- `mixin_yaml` 必填，且 YAML 根必须为 mapping。
- `extra_proxies_yaml` 允许空字符串；非空时 YAML 根必须为 sequence。
- `extra_proxy_providers_yaml` 允许空字符串；非空时 YAML 根必须为 mapping。

Normalization:

- 若 `mixin_yaml` 顶层包含 `proxies`（sequence）或 `proxy-providers`（mapping），服务端会自动抽取到
  `extra_proxies_yaml` / `extra_proxy_providers_yaml` 并从 `mixin_yaml` 移除；响应返回规范化后的结果
  （YAML 注释/anchors 不保证保留）。

Response `200`: 同 GET 结构。

Errors:

- `404 not_found`: user 不存在
- `400 invalid_request`: YAML 解析失败或根类型不匹配

## GET `/api/sub/{subscription_token}?format=mihomo`

- 当用户已配置 Mihomo profile：
  - 使用 `profile.mixin_yaml` 为静态基底渲染；
  - 系统重建并覆盖 `proxies`、`proxy-providers`；
  - relay 组 `🛣️ Japan|HongKong|Korea` 的 `use` 自动注入所有 provider 名称；
  - 系统覆盖并注入动态相关的 `proxy-groups`：
    - `🛣️ Japan|HongKong|Korea`
    - `🌟/🔒/🤯 {Japan|HongKong|Korea}`
    - `🛬 {base}` 与 `🔒 落地`
  - `proxy-providers` 可为空；为空时仍需输出可加载配置。
  - `extra_proxies_yaml` 中的节点会并入最终 `proxies`。
- 当用户未配置 Mihomo profile：回退到 clash 输出。

Response:

- `200 text/yaml; charset=utf-8`
