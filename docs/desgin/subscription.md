# xp · 订阅输出规格（URI / Base64 / Clash YAML）

## 1. 输出入口

- `GET /api/sub/{subscription_token}`：默认返回 Base64（便于大多数客户端直接导入）
- `GET /api/sub/{subscription_token}?format=raw`：返回纯 URI（逐行）
- `GET /api/sub/{subscription_token}?format=clash`：返回 Clash YAML（Mihomo/Clash.Meta）
- `GET /api/sub/{subscription_token}?format=mihomo`：canonical Mihomo URL；返回 provider 主配置（未配置 mixin 时回退 clash）
- `GET /api/sub/{subscription_token}/mihomo/legacy`：已移除，不再返回 Mihomo 主配置
- `GET /api/sub/{subscription_token}/mihomo/provider`：显式 provider Mihomo 主配置
- `GET /api/sub/{subscription_token}/mihomo/provider/system`：provider payload（`proxies:` YAML）

### Mihomo 外部资源镜像

- Mihomo 订阅工具栏和预览弹窗提供临时的 `Use XP mirror for external resources` 选项，默认使用原地址；Raw/Clash 不显示该选项。
- 勾选后只改写 GeoX、`rule-providers`、`proxy-providers` 的无认证 HTTPS URL，并通过 `external_resources=mirror` 传递给 canonical/provider URL。镜像 provider 固定使用 `proxy: DIRECT`，不转发自定义 Header。
- 镜像访问使用节点本地私网 CIDR 策略；部署默认来自 `XP_MIHOMO_ALLOWED_PRIVATE_CIDRS`，节点详情页可以为单个节点设置覆盖。策略入口接受裸私网 IP，并统一规范为主机 CIDR（IPv4 `/32`、原生 ULA IPv6 `/128`）；未获准的私网和特殊地址仍被拒绝，该策略对初始 URL 与每次重定向同时生效。
- XP 只为当前 profile 引用的 URL 和固定 MetaCubeX GeoX 资产建立一对一 HMAC 目录；没有任意 URL 代理参数。删除最后一个引用后资源 ID 立即失效。
- 镜像端点使用 256 MiB、90 秒、全局 32/单资源 4 的流式限制，不缓存内容、不落盘、不聚合响应；仅跟随无 userinfo 的 HTTPS 重定向，最多服务端跟随 5 次且不向客户端暴露跳转。

## 2. 统一规则

### 2.1 host 与端口

- `host`：使用 Endpoint 所属节点的 `Node.access_host`
- `port`：使用 Endpoint 的入站端口

### 2.2 命名（显示名）

- URI 的 `#name` 与 Clash 的 `name`：
  - 默认：`{user.display_name}-{node.node_name}-{endpoint.tag}`
  - `Grant.note` 可覆盖默认命名（更友好）
  - **同一订阅内要求 name 唯一**（避免 Clash/Mihomo 等客户端按 `name` 去重/覆盖导致“看起来缺了节点/端点”）：
    - `Grant.note` 为空/空白：使用默认命名（天然唯一：包含 `endpoint.tag`）
    - `Grant.note` 非空且在本订阅中唯一：使用 `Grant.note`（保持用户可读）
    - `Grant.note` 非空且在本订阅中出现多次：对这些冲突项使用 `{note}-{node.node_name}-{endpoint.tag}` 进行区分
- URI 的 `#name` 建议进行 URL encode（至少对空格等非法字符做转义），避免生成非法 URI（参见 SIP002 的说明）。

## 3. Raw URI（逐行）

Content-Type：`text/plain; charset=utf-8`

示例结构（占位符）：

### 3.1 VLESS + REALITY（vision/tcp）

```
vless://<UUID>@<HOST>:<PORT>?encryption=none&security=reality&type=tcp&sni=<SNI>&fp=<FP>&pbk=<PBK>&sid=<SID>&flow=xtls-rprx-vision#<NAME>
```

字段说明：

- `UUID`：Grant 的 vless uuid
- `HOST`：Node.access_host
- `PORT`：Endpoint.port
- `SNI`：Endpoint.reality.server_names 的首选项
- `FP`：Endpoint.reality.fingerprint（默认 `chrome`）
- `PBK`：Endpoint.reality.public_key（由 private_key 推导）
- `SID`：Endpoint.active_short_id（本项目禁止空 sid）

### 3.2 Shadowsocks 2022（tcp+udp）

本项目采用多用户 SS2022：客户端 password 为 `server_psk:user_psk`。

由于 SS2022 属于 AEAD-2022（SIP022），按 SIP002 约定 `userinfo` 不应进行 Base64URL 编码，而应使用 RFC3986 可解析的 “plain user info” 形式，并对 `method/password` 进行 percent-encoding。

```
ss://2022-blake3-aes-128-gcm:<PASSWORD>@<HOST>:<PORT>#<NAME>
```

其中：

- `method` 固定：`2022-blake3-aes-128-gcm`
- `PASSWORD`：对 `password` 进行 percent-encoding 后的字符串
- `password`：`<server_psk_b64>:<user_psk_b64>`（注意 `:`、`+`、`/`、`=` 等需编码）

## 4. Base64 订阅

Content-Type：`text/plain; charset=utf-8`

编码规则：

- 将 Raw URI 的完整文本（含换行）按 UTF-8 编码
- 整体进行 RFC4648 base64 编码
- 输出不换行（必要时由客户端自行处理）

## 5. Clash YAML（Mihomo/Clash.Meta）

Content-Type：`text/yaml; charset=utf-8`

### 5.1 VLESS（Reality）

> 下面字段名对齐 Mihomo/Meta 常用 schema：`reality-opts.public-key` 与 `reality-opts.short-id`，以及 `client-fingerprint`。

```yaml
proxies:
  - name: "<NAME>"
    type: vless
    server: "<HOST>"
    port: <PORT>
    uuid: "<UUID>"
    network: tcp
    udp: true
    tls: true
    flow: xtls-rprx-vision
    servername: "<SNI>"
    client-fingerprint: "<FP>"
    reality-opts:
      public-key: "<PBK>"
      short-id: "<SID>"
```

### 5.2 Shadowsocks 2022

```yaml
proxies:
  - name: "<NAME>"
    type: ss
    server: "<HOST>"
    port: <PORT>
    cipher: 2022-blake3-aes-128-gcm
    password: "<server_psk_b64>:<user_psk_b64>"
    udp: true
```

### 5.3 输出形态

MVP 建议输出“可直接导入”的最小 YAML：

- `proxies: [...]`
- 可选：追加一个 `proxy-groups`（例如 `select`）与基础 `rules`（后续再定，避免替用户做过多假设）

### 5.4 接入点 SMux 策略

- 仅 SS2022 的 XP 生成 YAML proxy 可带有接入点保存的 `smux` 配置；覆盖
  `format=clash`、Mihomo provider 主配置所引用的 system payload，以及 SS2022 的直连和
  链式条目。VLESS Reality/Vision 绝不输出 `smux`，因为 Mihomo `v1.19.29` 的 VLESS adapter
  不支持该字段。
- 该配置的最低客户端版本是 `Mihomo >= v1.19.29`。旧客户端应升级，或由管理员在
  SS2022 接入点高级设置关闭策略。
- 缺失 `meta.mihomo_smux` 的既有 SS2022 接入点默认启用：`protocol: smux`、
  `max-connections: 4`、`min-streams: 4`、`only-tcp: true`、`padding: false`、
  `statistic: false`。
- Raw/Base64 的 VLESS 与 Shadowsocks URI 不携带 SMux。SMux 是 SS2022 Mihomo YAML
  proxy 行为，不是 URI 的标准协商字段。

### 5.5 VLESS XHTTP/XMUX 连接复用

- VLESS Reality 新建接入点默认使用 `network: xhttp`、`alpn: [h2]` 与固定
  `xhttp-opts.path: /xp-xhttp`、`mode: stream-one`。这是 Mihomo XMUX 与 Xray
  SplitHTTP 的共同传输，不是顶层 `smux`。
- XHTTP YAML 的复用合同固定为每个接入点最多一条物理连接：
  `max-connections: "1"`、`max-concurrency: "0"`、
  `c-max-reuse-times: "0"`、`h-max-request-times: "0"`、
  `h-max-reusable-secs: "0"`、`h-keep-alive-period: -1`。负数关闭 Mihomo 默认的
  HTTP/2 PING；连接池完成预热后，
  并发代理流共享同一 HTTP/2 TCP；客户端首次冷启动的建连竞态不改变该稳态合同。
- XHTTP 覆盖 Clash、Mihomo provider 主配置引用的 system payload、VLESS 直连与链式条目。
  XHTTP 接入点的 Raw/Base64 URI 使用 `type=xhttp` 与 Mihomo v1.19.29 能识别的
  `extra.xmux` share-link 参数；legacy Vision/TCP URI 保持原样。
- `Mihomo >= v1.19.29` 与随 XP 发布的 `Xray >= v26.3.27` 是 XHTTP 最低兼容基线。旧客户端
  应升级，或在接入点高级设置中回退为 Vision/TCP 后刷新订阅。
- 历史 VLESS metadata 缺少 `transport` 时仍输出 Vision/TCP；管理员显式切换后服务端会重建
  inbound，必须刷新客户端 YAML/URI。详见 `../specs/endpoint-vless-xhttp-reuse/SPEC.md`。

## 6. Mihomo 混入配置输出（`format=mihomo`）

### 6.1 混入配置来源

- 混入配置按用户维度存储（admin API 管理），不内置到仓库。
- profile 字段：
  - `mixin_yaml`（必填，YAML root 必须是 mapping）
  - `extra_proxies_yaml`（可空；非空时 root 必须是 sequence）
  - `extra_proxy_providers_yaml`（可空；非空时 root 必须是 mapping）
- 保存 profile 时，`mixin_yaml` / `extra_proxies_yaml` / `extra_proxy_providers_yaml` 按原样分别存储。
- `PUT /api/admin/users/{user_id}/subscription-mihomo-profile` 只做 YAML 结构校验与“最终 provider 主配置 + system payload”联合预渲染校验；服务端不自动抽取、不自动规范化，也不隐式修复 legacy 引用。

### 6.2 Provider-only delivery

- `GET /api/sub/{subscription_token}?format=mihomo` 固定返回 provider 主配置。
- `/mihomo/provider` 返回同一 provider 主配置，便于回归。
- `/mihomo/legacy` 不再是可用订阅路径。
- provider 主配置中的系统 provider 名称固定为 `xp-system-generated`。

### 6.3 Provider 方案

Provider 方案将系统生成的代理放入 `xp-system-generated`，主配置只保留用户扩展与动态分组。系统节点通过 `/api/sub/{subscription_token}/mihomo/provider/system` 提供，主配置通过 `proxy-providers` 引用。

#### 6.3.1 用户模板插值

用户 `mixin_yaml` 的 `proxy-groups[*].proxies` 可以使用两个保留占位项：

- `🌟 <region>`：展开为当前用户可用的隐藏地区组，例如 `🌟 Japan`、`🌟 Singapore`。
- `🛬 <node>`：展开为当前用户每个实际落地节点的组，例如 `🛬 Tokyo-A`、`🛬 Singapore-B`。

占位项只在最终订阅渲染时展开；保存 profile 时保留原始文本。展开结果保留模板中其他字面量及其相对顺序。未订阅节点不得出现在任何展开结果中。

```yaml
proxy-groups:
  - name: "🎵 Spotify"
    type: select
    proxies:
      - "💎 节点选择"
      - "🌟 <region>"
      - "🛬 <node>"
      - "🎯 全球直连"
      - "🛑 全球拦截"
```

当用户拥有 Tokyo-A（Japan）和 Singapore-B（Singapore）时，最终应用组必须展开为：

```yaml
proxy-groups:
  - name: "🎵 Spotify"
    type: select
    proxies:
      - "💎 节点选择"
      - "🌟 Japan"
      - "🌟 Singapore"
      - "🛬 Tokyo-A"
      - "🛬 Singapore-B"
      - "🎯 全球直连"
      - "🛑 全球拦截"
```

应用组验证必须检查最终应用组自身的 `proxies`，不能只检查 `🚀 节点选择` 或 `🤯 All`。

#### 6.3.2 Provider 数据边界

- `proxy-providers` = `xp-system-generated` + `extra_proxy_providers_yaml`。
- 主配置 `proxies` = `extra_proxies_yaml`。
- 系统直连节点使用 `<node_slug>-reality` 与 `<node_slug>-ss` 命名。
- 系统链式节点使用 `<node_slug>-ss-chain` 与 `<node_slug>-reality-chain` 命名，并通过 `dialer-proxy` 指向对应的 `🛣️ {relay-base}`。
- `🔒 高质量`、`🔒 {Region}` 与落地组通过 `use: [xp-system-generated]` 消费系统 provider。
- Provider URL 使用请求对外 origin 构造，优先级为 `Forwarded`、`X-Forwarded-*`、`Host`，最后回退到 `api_base_url`。

#### 6.3.3 系统分组

系统只生成以下分组：

- 地区组：可见的 `🔒 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}` 与隐藏的 `🌟 {Region}`。
- 落地组：每个节点一个 `🛬 {base}`，以及汇总全部落地组的 `🔒 落地` `url-test` 组。
- 聚合组：`🔒 高质量`、`💎 高质量`、`🚀 节点选择`、`💎 节点选择` 与 `🤯 All`。
- 中转组：按 `Node.access_host` 聚合的隐藏 `🛣️ {relay-base}`。

`🤯 {Region}` 不生成。`🤯 All` 先列实际 `🛬 {base}`，再列七个 `🌟 {Region}`，按稳定顺序去重。`💎 高质量` 的 fallback 顺序固定为 `🔒 高质量`、`🔒 落地`、`🤯 All`。

每个存在 Reality access proxy 或链式候选的订阅节点都生成 `🛬 {base}`；如果存在 Reality access proxy，Reality 直连必须排在该落地组首位。`🚀 节点选择` 在落地组后追加 Reality 直连，不追加 `{base}-ss`。

地区归类以节点主动探测得到的 `subscription_region` 为主。尚未产生首次成功探测结果的历史节点可以使用 legacy slug fallback；未命中 fallback 的节点归入 `Other`。

### 6.4 Provider 渲染与校验

#### 6.4.1 渲染流程

1. 读取并保留用户 profile 中的 `mixin_yaml`、`extra_proxies_yaml` 与 `extra_proxy_providers_yaml`。
2. 生成系统 proxy、landing、relay、地区和聚合分组。
3. 将 `🌟 <region>` 与 `🛬 <node>` 展开为当前订阅实际存在的分组名。
4. 合并用户扩展、系统 provider 和最终分组。
5. 在最终主配置与 system provider payload 合并视图上执行引用校验。

渲染保留现有 legacy relay/region 引用的兼容归一化，但不修改用户存储值；占位项展开后，坏引用在最终校验阶段显式失败。

#### 6.4.2 Landing 与 Relay 规则

- `🛬 {base}` 使用精确 filter 匹配 `{base}-reality`、`{base}-ss-chain` 与 `{base}-reality-chain`。
- 同一 `access_host` 下的节点共享 `🛣️ {relay-base}`；不同 `access_host` 使用不同 relay 组。
- Relay 必须使用其他订阅节点的 `*-reality`，排除目标节点、链式节点和 `DIRECT`。
- 没有合法 relay 候选时使用 `REJECT` 与 `empty-fallback: REJECT`，不得因为外部 provider 存在而回退直连。
- `🔒 落地` 使用 `url-test`，成员必须完整覆盖最终生成的所有 `🛬 {base}`。

#### 6.4.3 输入与兼容校验

- Mixin 中的 `proxies` 与 `proxy-providers` 不直接作为系统节点来源；系统节点由 `xp-system-generated` 承载。
- 用户输入命中系统保留 proxy/provider 名称，或最终配置存在未定义引用时，返回 `400 invalid_request`。
- `GET /api/admin/users/{user_id}/subscription-mihomo-profile` 返回原始存储值；`PUT` 只执行 YAML 结构校验与最终渲染校验。
- Provider 主配置必须保留 `💎 高质量` 的兜底语义，不得退化为只包含 `🔒 高质量` 的单层入口。

### 6.5 脱敏 Provider 示例

- [provider 主配置示例](./examples/mihomo-reality-direct-provider-main.example.yaml) 展示顶层分组、
  `🚀 节点选择` 的 Reality-only provider filter，以及保留不变的用户自定义组。
- [system provider payload 示例](./examples/mihomo-reality-direct-system-provider.example.yaml) 展示
  `{base}-reality`、`{base}-ss`、`{base}-ss-chain` 与 `{base}-reality-chain` 的节点命名和链式引用。
- 两份文件均为脱敏拓扑样例，使用无效域名与占位凭据，不能用于实际连接。

### 6.6 缺失混入配置回退

- 若用户未配置 Mihomo profile，`format=mihomo` 回退到 `format=clash` 输出。
- 若用户在未配置 Mihomo profile 时请求 `external_resources=mirror`，返回 `422 invalid_request`，避免镜像选项无效却继续输出原始 Clash 配置。
