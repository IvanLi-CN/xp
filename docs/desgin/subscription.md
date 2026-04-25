# xp · 订阅输出规格（URI / Base64 / Clash YAML）

## 1. 输出入口

- `GET /api/sub/{subscription_token}`：默认返回 Base64（便于大多数客户端直接导入）
- `GET /api/sub/{subscription_token}?format=raw`：返回纯 URI（逐行）
- `GET /api/sub/{subscription_token}?format=clash`：返回 Clash YAML（Mihomo/Clash.Meta）
- `GET /api/sub/{subscription_token}?format=mihomo`：canonical Mihomo URL；返回 legacy 或 provider 主配置（未配置 mixin 时回退 clash）
- `GET /api/sub/{subscription_token}/mihomo/legacy`：显式 legacy Mihomo 主配置
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
- 保存 profile 时若 `mixin_yaml` 顶层包含 `proxies` / `proxy-providers`，服务端会自动抽取到
  `extra_proxies_yaml` / `extra_proxy_providers_yaml` 并从 `mixin_yaml` 移除（返回值为规范化后的文本；
  注释/anchors 不保证保留）。
- 若管理员同时在 `mixin_yaml` 顶层和对应 `extra_*` 字段里提供同类动态段，保存会返回 `invalid_request`，避免静默覆盖另一份输入。

### 6.2 双轨 delivery mode

- 新增全局持久化设置 `mihomo_delivery_mode=legacy|provider`，默认 `legacy`。
- `GET /api/sub/{subscription_token}?format=mihomo` 跟随该设置返回对应主配置。
- `/mihomo/legacy` 与 `/mihomo/provider` 始终返回固定方案，便于回归。
- provider 主配置中的系统 provider 名称固定为 `xp-system-generated`。

### 6.3 Provider 方案

- provider 方案中，系统直连 SS 节点（`{base}-ss`）移入 `GET /api/sub/{subscription_token}/mihomo/provider/system` 返回的 `proxies:` payload；系统 Reality 直连节点（`{base}-reality`）继续保留在主配置顶层 `proxies`，便于显式直连引用保持可见。
- provider 主配置顶层：
  - `proxy-providers` = `xp-system-generated` + `extra_proxy_providers_yaml`
  - `proxies` = `extra_proxies_yaml` + 系统 `{base}-reality` / `{base}-chain`
- `🛣️ JP/HK/TW` 与地区组继续通过 `use:` 消费 provider。
- 系统托管的地区面固定为 `🌟 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`，并同时生成 `🔒/🤯/🛣️ {Region}` 别名、`💎 高质量`、`🚀 节点选择` 与 `🤯 All`。
- 地区归类以节点主动探测出口公网 IP 后得到的 `subscription_region` 为主；但对尚未产生首次成功探测结果的历史节点，渲染阶段会先沿用 legacy slug fallback（仅覆盖 JP/HK/TW/KR）以避免升级瞬间清空原有地区组。首次成功探测落盘后，仅在 probe 未 stale 时继续把 `subscription_region` 视为权威；probe stale 后回退到 legacy slug fallback / `Other`。
- `🛬 {base}` 优先使用 Reality 直连：
  - 存在 `{base}-reality` 时，优先引用主配置顶层 `{base}-reality`，并在存在 `{base}-chain` 时把它作为回落候选
  - 仅当不存在 `{base}-reality` 且存在 `{base}-ss` 时，才保留 `{base}-chain` / `{base}-ss` 的旧回落路径
- provider URL 必须由请求对外 origin 构造（优先 `Forwarded` / `X-Forwarded-*` / `Host`，必要时回退 `api_base_url`）。
- provider 方案仍会隐藏系统 `{base}-ss` 直连，不承诺手写 `{base}-ss` 业务引用继续稳定；`{base}-reality` 的显式引用应保持可见且可用。

### 6.4 Legacy 渲染规则

- 渲染时忽略 mixin 中的 `proxies` 与 `proxy-providers`，由系统重建：
  - 系统节点：
    - reality direct：`<node_slug>-reality`
    - ss direct：`<node_slug>-ss`
    - ss chain：`<node_slug>-chain`，并设置 `dialer-proxy` 到单一外层候选组 `🛣️ JP/HK/TW`
  - 用户扩展：
    - 追加 `extra_proxies_yaml` 到最终 `proxies`
    - 以 `extra_proxy_providers_yaml` 作为最终 `proxy-providers`
- 名称冲突自动重命名（追加稳定后缀 `-dupN`）并记录告警日志。
- 所有 provider 名称会注入固定外层候选组 `🛣️ JP/HK/TW` 的 `use` 列表，并用单一 filter 在日本/香港/台湾节点中选最低延迟的外层入口。
- 系统会覆盖并注入一组“动态相关”的 `proxy-groups`（mixin config 不要求包含这些组定义）：
  - 外层候选组：`🛣️ JP/HK/TW`
  - 可见地区组：`🌟 {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`
  - 兼容地区组：`🔒/🤯/🛣️ {Japan|HongKong|Taiwan|Korea|Singapore|US|Other}`，保留名称但统一改为被动 `select` 组，避免恢复多地区主动测速
  - 聚合组：`💎 高质量`、`🚀 节点选择`、`🤯 All`
  - 落地组：`🛬 {base}` 与落地池 `🔒 落地`
- 地区组成员来自节点主动探测得到的 `subscription_region`；仅对尚未出现首次成功探测结果的历史节点保留 legacy slug fallback，未命中 fallback 的节点才落入 `🌟 Other`
- 对所有非系统、显式声明 `proxies` 的用户 `select` 组，若其 `proxies` 中引用了 `🛣️ JP/HK/TW` 或 legacy 地区组名，则最终输出会优先按模板 helper block（`proxy-group` / `proxy-group_with_relay` / `app-proxy-group`）的 `proxies` 顺序重放这些选项：系统管理地区名会折叠为可直接使用的 `🌟 {Japan|Korea|HongKong|Taiwan|Singapore|US|Other}`；若对应 helper 缺失，则退回到该组原始 `proxies` 顺序做最小替换。`🔒/🤯/🛣️ {Region}` 仍只作为内部隐藏组使用。
- `GET/PUT /api/admin/users/{user_id}/subscription-mihomo-profile` 返回的规范化结果会自动剥离系统托管引用（系统地区组、`🛬 *`、系统 `-ss/-reality/-chain`、失效 provider），用户模板仅保留偏好层与额外静态内容。
- 落地组生成策略：
  - 若存在 `{base}-reality`：优先放 `{base}-reality`，并在存在 `{base}-chain` 时把它作为回落候选
  - 否则若存在 `{base}-ss`：沿用 `{base}-chain` 与 `{base}-ss` 的兼容回落路径
- 旧 `-JP/-HK/-KR/-TW` 链式代理不再生成；旧链式引用会继续被裁剪，但地区组名会保留为兼容别名，并统一改成被动 `select` 组。
- Mihomo 不提供“纯被动、零主动探测”的自动回落；当前方案接受“失败后触发主动补检”，以换取显著减少主动测速带来的额外入站连接。

### 6.5 缺失混入配置回退

- 若用户未配置 Mihomo profile，`format=mihomo` 回退到 `format=clash` 输出。
