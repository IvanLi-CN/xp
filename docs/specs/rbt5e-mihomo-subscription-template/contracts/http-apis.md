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

- 无；请求与响应统一只接受 `mixin_yaml`。

Validation:

- `mixin_yaml` 必填，且 YAML 根必须为 mapping。
- `extra_proxies_yaml` 允许空字符串；非空时 YAML 根必须为 sequence。
- `extra_proxy_providers_yaml` 允许空字符串；非空时 YAML 根必须为 mapping。

Normalization:

- 若 `mixin_yaml` 顶层包含 `proxies`（sequence）或 `proxy-providers`（mapping），服务端会自动抽取到
  `extra_proxies_yaml` / `extra_proxy_providers_yaml` 并从 `mixin_yaml` 移除；响应返回规范化后的结果
  （YAML 注释/anchors 不保证保留）。
- 若同一类动态段同时在 `mixin_yaml` 顶层和对应 `extra_*` 字段里提供，服务端返回 `400 invalid_request`，避免静默覆盖。

Response `200`: 同 GET 结构。

Errors:

- `404 not_found`: user 不存在
- `400 invalid_request`: YAML 解析失败或根类型不匹配

## GET `/api/sub/{subscription_token}?format=mihomo`

- 当用户已配置 Mihomo profile：
  - 使用 `profile.mixin_yaml` 为静态基底渲染；
  - 系统重建并覆盖 `proxies`、`proxy-providers`；
  - 单一外层候选组 `🛣️ JP/HK/TW` 的 `use` 自动注入所有 provider 名称，并以日本/香港/台湾 filter 做 `url-test`；
  - 系统覆盖并注入动态相关的 `proxy-groups`：
    - `🛣️ JP/HK/TW`
    - `🛬 {base}` 与 `🔒 落地`
  - SS 接入点只生成 `{base}-ss` 与 `{base}-chain`；`{base}-chain` 的 `dialer-proxy` 固定指向 `🛣️ JP/HK/TW`。
  - 旧 `-JP/-HK/-KR/-TW` 代理引用与旧地区组名不做兼容映射；旧系统组定义会在渲染时剔除，最终输出会裁剪悬挂引用。
  - `proxy-providers` 可为空；为空时仍需输出可加载配置。
  - `extra_proxies_yaml` 中的节点会并入最终 `proxies`。
- 当用户未配置 Mihomo profile：回退到 clash 输出。

Response:

- `200 text/yaml; charset=utf-8`
