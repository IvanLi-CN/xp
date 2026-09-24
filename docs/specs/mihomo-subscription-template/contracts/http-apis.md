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
- 返回值按原样反映当前存储内容；服务端不做自动抽取或自动规范化。

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
- 若同一类动态段同时在 `mixin_yaml` 顶层和对应 `extra_*` 字段里提供，服务端返回 `400 invalid_request`，避免静默覆盖。
- 服务端按原样存储 `mixin_yaml` / `extra_*` 字段，不做自动抽取、不做自动规范化，也不隐式清洗系统托管引用。

Response `200`: 同 GET 结构。

Errors:

- `404 not_found`: user 不存在
- `400 invalid_request`: YAML 解析失败、根类型不匹配或 profile 联合渲染校验失败

## GET `/api/sub/{subscription_token}?format=mihomo`

- 当用户已配置 Mihomo profile：
  - 使用 `profile.mixin_yaml` 为静态基底渲染；
  - 系统重建并覆盖最终输出里的 `proxies`、`proxy-providers` 与系统托管 `proxy-groups`；
  - 系统注入的动态相关分组包括：
    - per-access-host relay 组 `🛣️ {relay-base}`
    - 可见地区组 `🌟 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`
    - 隐藏 alias `🔒/🤯 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`
    - 聚合组 `🔒 高质量`、`💎 高质量`、`🚀 节点选择`、`💎 节点选择`、`🤯 All`
    - 落地组 `🛬 {base}` 与 `🔒 落地`
- 地区分组以节点主动探测得到的 `subscription_region` 为主；但对尚未产生首次成功探测结果的历史节点，渲染阶段会先沿用 legacy slug fallback（仅覆盖 JP/HK/TW/KR）以避免升级瞬间清空原有地区组。首次成功探测落盘后，仅在 probe 未 stale 时继续把 `subscription_region` 视为权威；probe stale 后渲染回退到 legacy slug fallback / `Other`。
- 对非系统、显式声明 `proxies` 的用户 `select` 组，若其 `proxies` 引用了系统地区/聚合别名，则渲染结果会优先按模板 helper block（`proxy-group` / `proxy-group_with_relay` / `app-proxy-group`）的 `proxies` 顺序重放选项，并把系统管理地区名折叠为 owner-facing 的 `🌟 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`、`🔒 高质量`、`💎 节点选择`；`🔒/🤯 {Region}` 与 hidden alias 不直接暴露为这些用户组选项。
- `proxy-providers` 可为空；为空时仍需输出可加载配置。
- `extra_proxies_yaml` 中的节点会并入最终 `proxies`。
- 当用户未配置 Mihomo profile：回退到 clash 输出。

Response:

- `200 text/yaml; charset=utf-8`
