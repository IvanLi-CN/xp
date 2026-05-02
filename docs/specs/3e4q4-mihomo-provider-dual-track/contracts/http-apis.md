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
  "admin_token_masked": "********"
}
```

Notes:

- Mihomo delivery is provider-only; no runtime delivery mode is writable.
- 其它字段仍保持只读安全视图。

## GET `/api/sub/{subscription_token}?format=mihomo`

- canonical Mihomo URL。
- 当用户未配置 Mihomo profile 时，仍回退到 clash 输出。
- 当用户已配置 Mihomo profile 时，返回 provider 主配置。

Response:

- `200 text/yaml; charset=utf-8`

## GET `/api/sub/{subscription_token}/mihomo/legacy`

- legacy Mihomo route has been removed.

Response:

- `404 application/json`

## GET `/api/sub/{subscription_token}/mihomo/provider`

- 始终返回 provider 方案的 Mihomo 主配置。
- 当用户未配置 Mihomo profile 时，回退到 clash 输出。
- 顶层 `proxy-providers` 必含系统 provider `xp-system-generated` 与用户 `extra_proxy_providers_yaml`。
- 顶层 `proxies` 仅保留 `extra_proxies_yaml`；系统动态节点不写入主配置顶层。
- 系统 provider 的 `url` 必须指向当前请求对外 origin 下的 `/api/sub/{token}/mihomo/provider/system`。
- `🛬 {base}` 通过 `use: [xp-system-generated]` 与精确 `filter` 消费 `{base}-ss-chain` / `{base}-reality-chain`，且 Mihomo 运行时按 ss-chain、reality-chain 顺序展示。
- `🔒 {Region}` 通过 `use: [xp-system-generated]` 与 `filter` 动态包含 `{base}-reality` 接入点；`{base}-ss` 不作为本次地区接入点目标。
- `🛣️ JP/HK/TW` 只消费外部第三方 provider；无外部 provider 时回落 `DIRECT`，不得 `use` `xp-system-generated`。
- 系统托管的可见地区组固定为 `🌟 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`，并同时生成 `💎 高质量`、`🚀 节点选择` 与 `🤯 All`；这些组以节点主动探测得到的 `subscription_region` 为主，但对尚未产生首次成功探测结果的历史节点，渲染阶段会保留 legacy slug fallback（仅覆盖 JP/HK/TW/KR）以兼容滚动升级；probe stale 后同样回退到 legacy slug fallback / `Other`。

Response:

- `200 text/yaml; charset=utf-8`

## GET `/api/sub/{subscription_token}/mihomo/provider/system`

- 返回系统 provider payload，根为：

```yaml
proxies:
  - name: tokyo-ss
    type: ss
    # ...
  - name: tokyo-reality-chain
    type: vless
    dialer-proxy: 🛣️ JP/HK/TW
    # ...
```

Rules:

- 包含系统直连与链式节点：`{base}-ss`、`{base}-reality`、`{base}-ss-chain`、`{base}-reality-chain`。
- 同一 `{base}` 在 provider payload 中应稳定排序，使 `🛬 {base}` 过滤链式节点后的候选顺序为 `{base}-ss-chain`、`{base}-reality-chain`。
- provider payload 可被 Mihomo `proxy-providers.type=http` 直接消费。
- 不依赖用户是否配置 Mihomo profile；即使主配置路径因缺少 profile 回退 clash，system payload 路径仍可单独返回系统隐藏直连节点。
- 新节点一旦拥有 system payload entry 且主动探测得到地区归类，就会自动通过 provider filter 出现在地区组 / `💎 高质量` / `🚀 节点选择` 中，无需更新用户模板。

Response:

- `200 text/yaml; charset=utf-8`

Errors:

- `404 not_found`: token 不存在
- `400 invalid_request`: provider 保留名冲突或其它用户配置不可恢复错误
