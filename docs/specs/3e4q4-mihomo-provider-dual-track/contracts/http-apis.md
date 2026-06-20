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
  "vless_https_canary_bind": "127.0.0.1:39043",
  "vless_https_canary_acme_directory_url": "https://acme-v02.api.letsencrypt.org/directory",
  "vless_https_canary_status": {
    "enabled": true,
    "bind": "127.0.0.1:39043",
    "acme_directory_url": "https://acme-v02.api.letsencrypt.org/directory",
    "cert_not_after": "RFC3339|null",
    "last_renewed_at": "RFC3339|null",
    "last_error": "string|null"
  },
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
- `vless_https_canary_status` is additive runtime observability: it reports the loopback TLS canary / ACME state and must not be treated as a writable config payload.
- 其它字段仍保持只读安全视图。

## GET `/api/health`

Response `200`:

```json
{
  "status": "ok",
  "xray": {
    "status": "unknown|up|down"
  },
  "cloudflared": {
    "status": "disabled|unknown|up|down"
  },
  "vless_https_canary": {
    "enabled": true,
    "bind": "127.0.0.1:39043",
    "acme_directory_url": "https://acme-v02.api.letsencrypt.org/directory",
    "cert_not_after": "RFC3339|null",
    "last_renewed_at": "RFC3339|null",
    "last_error": "string|null"
  }
}
```

Notes:

- `vless_https_canary` is additive and backward-compatible. Existing health consumers should keep treating top-level `status` as the liveness contract.
- When the canary is unavailable, `vless_https_canary.enabled=false` and the optional fields may be omitted.

## GET `/api/sub/{subscription_token}?format=mihomo`

- canonical Mihomo URL。
- 当用户未配置 Mihomo profile 时，仍回退到 clash 输出。
- 当用户已配置 Mihomo profile 时，返回 provider 主配置。
- `PUT /api/admin/users/{user_id}/subscription-mihomo-profile` 会先对最终 provider 主配置 + `/mihomo/provider/system` payload 做联合预渲染校验；任何未定义的 `proxies`、`use`、`dialer-proxy` 或 `rules` 引用都会返回 `400 invalid_request`。

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
- `🔒 高质量` 与 `🔒 {Region}` 通过 `use: [xp-system-generated]` 与 `filter` 动态包含 `{base}-reality` 接入点，并通过 `exclude-filter` 排除系统 `{base}-ss` 直连接入候选。
- `💎 高质量` 作为 owner-facing 高质量入口不得失去兜底层；最终主配置必须稳定提供“高质量入口 + 全局兜底入口”两层语义。若 `💎 高质量` 本身不直接引用 `🤯 All`，则必须存在另一个稳定 owner-facing 包装组同时暴露 `💎 高质量` 与 `🤯 All`，不能让最终入口仅剩 `🔒 高质量` 单一路径。
- per-base relay 组 `🛣️ {relay-base}` 按 `Node.access_host` 聚合生成；同一 `access_host` 下的落地节点共享一个 relay 组，不同 `access_host` 生成不同 relay 组。`relay-base` 必须保留 access host 分隔符差异，避免 `a.b.example.com` / `a-b.example.com` 这类 host 退化成同一 slug 后按当前集合计数重命名。若 `relay-base` 等于历史地区 alias 基名，则输出必须加内部前缀消歧，不得重新生成 `🛣️ {Region}`。
- per-base relay 组只消费外部第三方 provider；无外部 provider 时回落 `DIRECT`，不得 `use` `xp-system-generated`。有外部 provider 时通过日本/香港/新加坡 filter 做 `url-test` 主动探测，并保留 `DIRECT` 兜底以防 provider 候选被 filter 筛空；健康检查 URL 的选择顺序是：最小托管 VLESS 端口对应的 `https://<access_host[:port]>/generate_204` -> 唯一公开 `api_base_url + /api/health` -> `https://www.gstatic.com/generate_204`。
- 系统托管地区组的最终形态由 [final-mihomo-config.md](./final-mihomo-config.md) 定义：`🔒 {Region}` 是 visible leaf `select`，`🌟 {Region}` 是 hidden `fallback` 包装，`🤯 {Region}` 是 hidden `url-test` 包装；这些组以节点主动探测得到的 `subscription_region` 为主，但对尚未产生首次成功探测结果的历史节点，渲染阶段会保留 legacy slug fallback（仅覆盖 JP/HK/TW/KR）以兼容滚动升级；probe stale 后同样回退到 legacy slug fallback / `Other`。
- 输出不得生成共享 `🛣️ JP/HK/SG` 主路径或 `🛣️ {Region}` 兼容地区别名；旧共享外层与旧地区 relay alias 引用只允许被清理或移除。
- PUT 保存阶段不会自动抽取 `mixin_yaml.proxies` / `mixin_yaml.proxy-providers`，也不会把 legacy relay alias、旧 landing 引用或保留名冲突做隐含转换。
- hidden per-base relay 组必须统一移动到系统托管组尾部，不能插在 `💎 高质量` 与地区组之间。
- `💎 高质量` 的兜底语义不依赖 mixin 是否显式声明 `🤯 All`；如果最终输出缺少面向 owner 的全局兜底层，视为渲染合同缺失。

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
    dialer-proxy: 🛣️ tokyo
    # ...
```

Rules:

- 包含系统直连与链式节点：`{base}-ss`、`{base}-reality`、`{base}-ss-chain`、`{base}-reality-chain`。
- `{base}-ss-chain` 与 `{base}-reality-chain` 的 `dialer-proxy` 必须指向该节点 `access_host` 对应的 per-base relay 组；同一 `access_host` 的多个 base 共享同一个 relay 组名。
- 同一 `{base}` 在 provider payload 中应稳定排序，使 `🛬 {base}` 过滤链式节点后的候选顺序为 `{base}-ss-chain`、`{base}-reality-chain`。
- provider payload 可被 Mihomo `proxy-providers.type=http` 直接消费。
- 不依赖用户是否配置 Mihomo profile；即使主配置路径因缺少 profile 回退 clash，system payload 路径仍可单独返回系统隐藏直连节点。
- 新节点一旦拥有 system payload entry 且主动探测得到地区归类，就会自动通过 provider filter 出现在地区组 / `💎 高质量` / `🚀 节点选择` 中，无需更新用户模板。

Response:

- `200 text/yaml; charset=utf-8`

Errors:

- `404 not_found`: token 不存在
- `400 invalid_request`: provider 保留名冲突或其它用户配置不可恢复错误
