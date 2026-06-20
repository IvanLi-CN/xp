# xp · 订阅输出规格（URI / Base64 / Clash YAML）

## 1. 输出入口

- `GET /api/sub/{subscription_token}`：默认返回 Base64（便于大多数客户端直接导入）
- `GET /api/sub/{subscription_token}?format=raw`：返回纯 URI（逐行）
- `GET /api/sub/{subscription_token}?format=clash`：返回 Clash YAML（Mihomo/Clash.Meta）
- `GET /api/sub/{subscription_token}?format=mihomo`：canonical Mihomo URL；返回 provider 主配置（未配置 mixin 时回退 clash）
- `GET /api/sub/{subscription_token}/mihomo/legacy`：已移除，不再返回 Mihomo 主配置
- `GET /api/sub/{subscription_token}/mihomo/provider`：显式 provider Mihomo 主配置
- `GET /api/sub/{subscription_token}/mihomo/provider/system`：provider payload（`proxies:` YAML）

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

- provider 方案中，系统直连节点（`{base}-ss` / `{base}-reality`）与链式节点（`{base}-ss-chain` / `{base}-reality-chain`）都由 `GET /api/sub/{subscription_token}/mihomo/provider/system` 返回的 `proxies:` payload 动态承载。
- provider 主配置顶层：
  - `proxy-providers` = `xp-system-generated` + `extra_proxy_providers_yaml`
  - `proxies` = `extra_proxies_yaml`
- `🔒 高质量` 与地区组继续通过 `use:` 消费 provider；`🔒 高质量` / `🔒 {Region}` 必须能动态包含系统 `{base}-reality` 接入点，`{base}-ss` 不作为接入点目标。
- per-base relay 组按 `Node.access_host` 聚合生成，命名为 `🛣️ {relay-base}`；同一 `access_host` 下的多个落地节点共享一个 relay 组，不同 `access_host` 生成不同 relay 组。`relay-base` 的 host slug 会保留 `.` 与 `-` 等分隔符差异，避免 `a.b.example.com` / `a-b.example.com` 这类 host 随当前订阅集合发生计数式重命名；若等于历史地区 alias 基名，会加内部前缀消歧，避免重新输出 `🛣️ {Region}`。
- per-base relay 组只消费外部第三方 provider，避免系统 `*-chain` 递归指回自身；有外部 provider 时通过日本/香港/新加坡 filter 做 `url-test`，并保留 `DIRECT` 兜底以防 provider 候选被 filter 筛空。无外部 provider 时同样回落 `DIRECT`。健康检查 URL 选择顺序为：
  - 同一 `access_host` 下存在托管 VLESS endpoint 时，选择最小 VLESS 端口，并使用 `https://<access_host[:port]>/generate_204`
  - 否则当同组只有一个公开 `api_base_url` 时，使用 `<api_base_url>/api/health`
  - 否则使用 Mihomo 通用 `https://www.gstatic.com/generate_204`
- 系统托管的地区面固定为：visible leaf `🔒 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`、hidden `fallback` 包装 `🌟 {Region}`、hidden `url-test` 包装 `🤯 {Region}`；同时生成 `🔒 高质量`、`💎 高质量`、`🚀 节点选择`、`💎 节点选择` 与 `🤯 All`。
- `💎 高质量` 必须保留 owner-facing 兜底层语义，不能退化成只剩 `🔒 高质量` 的单层入口；若 `💎 高质量` 本身不直接挂 `🤯 All`，则最终输出必须另有一个稳定包装入口同时暴露 `💎 高质量` 与 `🤯 All`。
- 地区归类以节点主动探测出口公网 IP 后得到的 `subscription_region` 为主；但对尚未产生首次成功探测结果的历史节点，渲染阶段会先沿用 legacy slug fallback（仅覆盖 JP/HK/TW/KR）以避免升级瞬间清空原有地区组。首次成功探测落盘后，仅在 probe 未 stale 时继续把 `subscription_region` 视为权威；probe stale 后回退到 legacy slug fallback / `Other`。
- `🛬 {base}` 通过 `use: [xp-system-generated]` 与精确 `filter` 消费 `{base}-ss-chain` / `{base}-reality-chain`，并依赖 system provider payload 的稳定排序让 Mihomo 运行时按 ss-chain、reality-chain 顺序展示。
- provider URL 必须由请求对外 origin 构造（优先 `Forwarded` / `X-Forwarded-*` / `Host`，必要时回退 `api_base_url`）。
- provider 方案隐藏系统直连节点，不承诺手写 `{base}-ss` / `{base}-reality` 业务引用继续稳定。

### 6.4 Provider 渲染规则

- 渲染时忽略 mixin 中的 `proxies` 与 `proxy-providers`，由系统重建：
  - 系统节点：
    - reality direct：`<node_slug>-reality`
    - ss direct：`<node_slug>-ss`
    - ss chain：`<node_slug>-ss-chain`，并设置 `dialer-proxy` 到该节点 `access_host` 对应的 per-base relay 组 `🛣️ {relay-base}`
    - reality chain：`<node_slug>-reality-chain`，并设置 `dialer-proxy` 到该节点 `access_host` 对应的 per-base relay 组 `🛣️ {relay-base}`
  - 用户扩展：
    - 追加 `extra_proxies_yaml` 到主配置顶层 `proxies`
    - 以 `extra_proxy_providers_yaml` 追加到最终 `proxy-providers`
- 用户输入若命中系统保留 proxy / provider 名称，或最终配置中存在未定义引用，保存阶段直接返回 `400 invalid_request`；服务端不做自动重命名。
- 所有外部 provider 名称会注入每个 per-base relay 组的 `use` 列表，并用日本/香港/新加坡 filter 选择外层中转节点。
- 系统会覆盖并注入一组“动态相关”的 `proxy-groups`（mixin config 不要求包含这些组定义）：
  - per-base relay 组：`🛣️ {relay-base}`，按 `Node.access_host` 聚合，同机共享
- hidden fallback 地区组：`🌟 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`
- 可见地区组：`🔒 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`
- hidden probe 组：`🤯 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`；`🛣️ {Region}` 兼容别名不再生成
- 聚合组：`🔒 高质量`、`💎 高质量`、`🚀 节点选择`、`💎 节点选择`、`🤯 All`
  - 落地组：`🛬 {base}` 与落地池 `🔒 落地`
- 地区组成员来自节点主动探测得到的 `subscription_region`；仅对尚未出现首次成功探测结果的历史节点保留 legacy slug fallback，未命中 fallback 的节点才落入 `🌟 Other`
- 最终输出不再对用户 profile 做 helper replay、legacy relay remap、legacy landing remap 或系统托管引用剥离；用户输入原样存储，坏数据只在最终 provider 主配置 + system payload 联合校验阶段显式失败。
- `GET /api/admin/users/{user_id}/subscription-mihomo-profile` 返回原始存储值；`PUT` 只做 YAML 结构校验与最终渲染校验，不做自动抽取或规范化。
- `🔒 高质量` 若由用户模板提供，provider 渲染会为其追加 `xp-system-generated` 并用 `filter` / `exclude-filter` 显式放行系统 `{base}-reality`、排除系统 `{base}-ss`，保留原有外部 provider 语义。
- 落地组生成策略：只通过 provider `use + filter` 匹配 `{base}-ss-chain` / `{base}-reality-chain`；同一 base 的 system provider payload 顺序必须保证过滤后 ss-chain 在 reality-chain 前。
- hidden per-base relay 组 `🛣️ {relay-base}` 会在最终 `proxy-groups` 中统一移动到系统托管组尾部，位于地区组、`🛬 {base}`、`🔒 落地`、`🤯 All`、`🚀 节点选择` 之后。
- `💎 高质量` 的兜底要求不依赖用户 mixin 是否显式写入 `🤯 All`；这是系统输出合同本身的一部分。
- 旧 `-JP/-HK/-KR/-TW` 链式代理不再生成；旧链式引用会继续被裁剪；地区组合同固定为 `🔒 {Region}` visible select leaf、`🌟 {Region}` hidden `fallback` wrapper、`🤯 {Region}` hidden `url-test` wrapper。
- `/api/health` 与 `/api/admin/config` 都会增量暴露 `vless_https_canary` 运行态，便于运维审计证书有效期、loopback bind 与最近一次续期错误；这些字段只读。
- Mihomo 不提供“纯被动、零主动探测”的自动回落；当前方案接受“失败后触发主动补检”，以换取显著减少主动测速带来的额外入站连接。

### 6.5 缺失混入配置回退

- 若用户未配置 Mihomo profile，`format=mihomo` 回退到 `format=clash` 输出。
