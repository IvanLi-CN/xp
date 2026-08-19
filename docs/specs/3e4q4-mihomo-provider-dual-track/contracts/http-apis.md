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
- 请求 `external_resources=mirror` 但用户未配置 Mihomo profile 时返回 `422 invalid_request`，避免镜像选择静默失效。
- 当用户已配置 Mihomo profile 时，返回 provider 主配置。
- `PUT /api/admin/users/{user_id}/subscription-mihomo-profile` 会先对最终 provider 主配置 + `/mihomo/provider/system` payload 做联合预渲染校验；任何未定义的 `proxies`、`use`、`dialer-proxy` 或 `rules` 引用都会返回 `400 invalid_request`。
- `external_resources=mirror` 是临时输出选项。仅在 Mihomo 格式有效；省略时保持原始 URL。
- 镜像模式只改写 GeoX、`rule-providers` 和 `proxy-providers` 中的 HTTPS URL，并为缺失的 GeoX 键注入固定 MetaCubeX 资产集。镜像 provider 强制 `proxy: DIRECT`，不转发用户自定义 Header。
- 非 HTTPS、带 URL userinfo 或带自定义 Header 的资源返回 `422 invalid_request`；原地址模式不受影响。

Response:

- `200 text/yaml; charset=utf-8`

## GET `/api/sub/{subscription_token}/mihomo/legacy`

- legacy Mihomo route has been removed.

Response:

- `404 application/json`

## GET `/api/sub/{subscription_token}/mihomo/provider`

- 始终返回 provider 方案的 Mihomo 主配置。
- 当用户未配置 Mihomo profile 时，回退到 clash 输出。
- 请求 `external_resources=mirror` 但用户未配置 Mihomo profile 时返回 `422 invalid_request`，避免镜像选择静默失效。
- 顶层 `proxy-providers` 必含系统 provider `xp-system-generated` 与用户 `extra_proxy_providers_yaml`。
- 顶层 `proxies` 仅保留 `extra_proxies_yaml`；系统动态节点不写入主配置顶层。
- 系统 provider 的 `url` 必须指向当前请求对外 origin 下的 `/api/sub/{token}/mihomo/provider/system`。
- `🛬 {base}` 仅在存在链式候选时生成，并通过 `use: [xp-system-generated]` 与精确 `filter`
  消费 `{base}-reality`、`{base}-ss-chain` / `{base}-reality-chain`，且 Mihomo 运行时按
  reality、ss-chain、reality-chain 顺序展示。
- `🚀 节点选择` 在全部 `🛬 {base}` 后追加 Reality 直连候选，不追加 `{base}-ss`。非 provider
  输出直接列出 `{base}-reality`，provider 主配置则通过 `use: [xp-system-generated]` 和精确
  `filter` 直接暴露对应 `{base}-reality`。
- `🔒 高质量` 与 `🔒 {Region}` 通过 `use: [xp-system-generated]` 与 `filter` 动态包含 `{base}-reality` 接入点，并通过 `exclude-filter` 排除系统 `{base}-ss` 直连接入候选。
- `💎 高质量` 作为 owner-facing 高质量入口不得失去兜底层；最终主配置必须稳定提供“高质量入口 + 全局兜底入口”两层语义。若 `💎 高质量` 本身不直接引用 `🤯 All`，则必须存在另一个稳定 owner-facing 包装组同时暴露 `💎 高质量` 与 `🤯 All`，不能让最终入口仅剩 `🔒 高质量` 单一路径。
- per-base relay 组 `🛣️ {relay-base}` 按 `Node.access_host` 聚合生成；同一 `access_host` 下的落地节点共享一个 relay 组，不同 `access_host` 生成不同 relay 组。`relay-base` 必须保留 access host 分隔符差异，避免 `a.b.example.com` / `a-b.example.com` 这类 host 退化成同一 slug 后按当前集合计数重命名。若 `relay-base` 等于历史地区 alias 基名，则输出必须加内部前缀消歧，不得重新生成 `🛣️ {Region}`。
- per-base relay 组只消费外部第三方 provider，不得使用 `DIRECT` 兜底，也不得 `use` `xp-system-generated`。
  有外部 provider 时通过日本/香港/新加坡 filter 做 `url-test` 主动探测，并显式保留 `REJECT`
  哨兵覆盖过滤为空或初始化状态；无外部 provider 时 relay 组只能使用 `REJECT` 拒绝哨兵，
  provider 候选被 filter 筛空时不得回落直连或暴露 `COMPATIBLE`。健康检查 URL 的选择顺序是：
  最小托管 VLESS 端口对应的 `https://<access_host[:port]>/generate_204` -> 唯一公开
  `api_base_url + /api/health` -> `https://www.gstatic.com/generate_204`。
- `POST /api/admin/endpoints` 在 `kind=vless_reality_vision_tcp` 时支持两种形状：
  legacy 非托管创建继续显式提交 `reality`；托管创建则省略 `reality`，只允许可选
  `canary_upstream` 与 `accepted_authorities`。托管创建成功后，服务端必须派生
  `reality.dest=XP_VLESS_CANARY_BIND`、`server_names=[node.access_host]`、
  `server_names_source=manual`、`fingerprint=chrome`，并把 endpoint 标记为
  `managed_default=true`。
- 托管 VLESS HTTPS canary 的公共反代面只接受 canonical `access_host[:endpoint_port]` 和 endpoint 自身声明的 `accepted_authorities` 别名集合。`accepted_authorities` 是普通 HTTPS `host[:port]` 无序集合；省略端口时按 HTTPS 默认 `443` 解释。它只影响 Host 匹配，不影响 REALITY `server_names`、`reality.dest` 或 provider 生成的 canonical `/generate_204` URL。未命中、authority 冲突和缺少 `canary_upstream` 的公共响应统一收敛为纯文本 `404 Not Found`。
- 系统托管地区组的最终形态由 [final-mihomo-config.md](./final-mihomo-config.md) 定义：`🔒 {Region}` 是 visible leaf `select`，`🌟 {Region}` 是 hidden `fallback` 包装，`🤯 {Region}` 是 hidden `url-test` 包装；这些组以节点主动探测得到的 `subscription_region` 为主，但对尚未产生首次成功探测结果的历史节点，渲染阶段会保留 legacy slug fallback（仅覆盖 JP/HK/TW/KR）以兼容滚动升级；probe stale 后同样回退到 legacy slug fallback / `Other`。
- 输出不得生成共享 `🛣️ JP/HK/SG` 主路径或 `🛣️ {Region}` 兼容地区别名；旧共享外层与旧地区 relay alias 引用只允许被清理或移除。
- PUT 保存阶段不会自动抽取 `mixin_yaml.proxies` / `mixin_yaml.proxy-providers`，也不会把 legacy relay alias、旧 landing 引用或保留名冲突做隐含转换。
- hidden per-base relay 组必须统一移动到系统托管组尾部，不能插在 `💎 高质量` 与地区组之间。
- `💎 高质量` 的兜底语义不依赖 mixin 是否显式声明 `🤯 All`；如果最终输出缺少面向 owner 的全局兜底层，视为渲染合同缺失。
- `external_resources=mirror` 与 canonical 订阅使用相同的镜像改写合同。

Response:

- `200 text/yaml; charset=utf-8`

## GET `/api/mihomo/resources/{resource_id}`

- `resource_id` 是使用现有集群持久化密钥对规范化原 URL 做 HMAC-SHA-256 后的十六进制值。
- 目录只包含当前 Mihomo profile 仍引用的 GeoX、规则 provider、代理 provider URL，以及 XP 固定 GeoX 资产；不接受 `url` 查询参数或其它任意上游地址。
- 资源内容不在 XP 缓存、不写磁盘、不聚合到内存；上游响应按块流式转发。XP 施加 256 MiB、90 秒总超时、全局 32 条和单资源 4 条并发限制。
- 初始 URL 和每个重定向目标都必须是无 userinfo 的 HTTPS；重定向在服务端最多跟随 5 次，客户端永远看不到 `Location`。
- 集群设置 `allow_private_targets=false`（默认）时，XP 会对初始 URL 和每个重定向目标做 DNS 解析并固定到解析结果，拒绝 loopback、私网、链路本地、保留和文档地址，返回 `403 private_target_blocked`。开启后保留原有的最终目标直连语义。

Responses:

- `200`: upstream success, streamed body
- `404`: resource ID 不在当前目录（删除最后一个引用后立即失效）
- `413`: upstream 声明超过 256 MiB
- `429`: 并发闸门已满，带 `Retry-After: 1`
- `502`: DNS、连接、TLS 或重定向解析失败
- `504`: 90 秒总超时
- `508`: 第六次重定向
- `403`: 集群策略拒绝私网、回环或链路本地目标
- upstream `4xx/5xx`: 保留状态码，使用 XP 固定错误体且不透传上游响应体或 `Location`

## GET `/api/admin/mihomo/resource-policy`

- 需要管理员认证。
- 返回当前集群级外部资源镜像策略：

```json
{"allow_private_targets": false}
```

## PUT `/api/admin/mihomo/resource-policy`

- 需要管理员认证。
- 请求体：

```json
{"allow_private_targets": true}
```

- 设置通过 Raft 持久化并立即作用于所有节点的公开镜像请求。

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
- 同一 `{base}` 在 provider payload 中应稳定排序，使 `🛬 {base}` 过滤后的候选顺序为
  `{base}-reality`、`{base}-ss-chain`、`{base}-reality-chain`。
- `🚀 节点选择` 通过精确 Reality filter 在全部 `🛬 {base}` 后追加 `{base}-reality`，不追加
  `{base}-ss`。
- provider payload 可被 Mihomo `proxy-providers.type=http` 直接消费。
- 不依赖用户是否配置 Mihomo profile；即使主配置路径因缺少 profile 回退 clash，system payload 路径仍可单独返回系统隐藏直连节点。
- 新节点一旦拥有 system payload entry 且主动探测得到地区归类，就会自动通过 provider filter 出现在地区组 / `💎 高质量` / `🚀 节点选择` 中，无需更新用户模板。

Response:

- `200 text/yaml; charset=utf-8`

Errors:

- `404 not_found`: token 不存在
- `400 invalid_request`: provider 保留名冲突或其它用户配置不可恢复错误
