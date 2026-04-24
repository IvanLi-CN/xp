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
- 系统保留动态引用（`🌟/🔒/🤯/🛣️ {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`、`🛬 *`、系统 `-ss/-reality/-chain`、失效 provider）会在 GET/PUT 规范化结果中自动剥离，不再作为用户模板的稳定输入。

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
    - `🌟/🔒/🤯/🛣️ {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`
    - `💎 高质量`、`🚀 节点选择`、`🤯 All`
    - `🛬 {base}` 与 `🔒 落地`
- 地区分组以节点主动探测得到的 `subscription_region` 为主；但对尚未产生首次成功探测结果的历史节点，渲染阶段会先沿用 legacy slug fallback（仅覆盖 JP/HK/TW/KR）以避免升级瞬间清空原有地区组。首次成功探测落盘后，仅在 probe 未 stale 时继续把 `subscription_region` 视为权威；probe stale 后渲染回退到 legacy slug fallback / `Other`。
  - 对非系统、显式声明 `proxies` 的用户 `select` 组，若其 `proxies` 引用了 `🛣️ JP/HK/TW` 或 legacy 地区组名，则渲染结果会优先按模板 helper block（`proxy-group` / `proxy-group_with_relay` / `app-proxy-group`）的 `proxies` 顺序重放选项，并把系统管理地区名折叠为可直接使用的 `🌟 {Japan|Korea|HongKong|Taiwan|Singapore|US|Other}`；若对应 helper 缺失，则退回原始 `proxies` 顺序做最小替换。`🔒/🤯/🛣️ {Region}` 不直接暴露为这些用户组选项。
  - SS 接入点只生成 `{base}-ss` 与 `{base}-chain`；`{base}-chain` 的 `dialer-proxy` 固定指向 `🛣️ JP/HK/TW`。
  - 旧 `-JP/-HK/-KR/-TW` 代理引用与旧地区组名不做兼容映射；旧系统组定义会在渲染时剔除，最终输出会裁剪悬挂引用。
  - `proxy-providers` 可为空；为空时仍需输出可加载配置。
  - `extra_proxies_yaml` 中的节点会并入最终 `proxies`。
- 当用户未配置 Mihomo profile：回退到 clash 输出。

Response:

- `200 text/yaml; charset=utf-8`
