# xp · 订阅输出规格（URI / Base64 / Clash YAML）

## 1. 输出入口

- `GET /api/sub/{subscription_token}`：默认返回 Base64（便于大多数客户端直接导入）
- `GET /api/sub/{subscription_token}?format=raw`：返回纯 URI（逐行）
- `GET /api/sub/{subscription_token}?format=clash`：返回 Clash YAML（Mihomo/Clash.Meta）
- `GET /api/sub/{subscription_token}?format=mihomo`：返回用户模板驱动的完整 Mihomo YAML（未配置模板时回退 clash）

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

## 6. Mihomo 模板驱动输出（`format=mihomo`）

### 6.1 模板来源

- 模板按用户维度存储（admin API 管理），不内置到仓库。
- profile 字段：
  - `template_yaml`（必填，YAML root 必须是 mapping）
  - `extra_proxies_yaml`（可空；非空时 root 必须是 sequence）
  - `extra_proxy_providers_yaml`（可空；非空时 root 必须是 mapping）
- 保存 profile 时若 `template_yaml` 顶层包含 `proxies` / `proxy-providers`，服务端会自动抽取到
  `extra_proxies_yaml` / `extra_proxy_providers_yaml` 并从 `template_yaml` 移除（返回值为规范化后的文本；
  注释/anchors 不保证保留）。

### 6.2 渲染规则

- 渲染时忽略模板中的 `proxies` 与 `proxy-providers`，由系统重建：
  - 系统节点：
    - reality direct：`<node_slug>-reality`
    - ss direct：`<node_slug>-ss`
    - ss chain：`<node_slug>-JP|HK|KR`，并设置 `dialer-proxy` 到 `🛣️ Japan|HongKong|Korea`
  - 用户扩展：
    - 追加 `extra_proxies_yaml` 到最终 `proxies`
    - 以 `extra_proxy_providers_yaml` 作为最终 `proxy-providers`
- 名称冲突自动重命名（追加稳定后缀 `-dupN`）并记录告警日志。
- 所有 provider 名称会注入固定 relay 组 `🛣️ Japan|🛣️ HongKong|🛣️ Korea` 的 `use` 列表。
- 系统会覆盖并注入一组“动态相关”的 `proxy-groups`（mixin config 不要求包含这些组定义）：
  - relay 组：`🛣️ Japan|🛣️ HongKong|🛣️ Korea`
  - 地区入口组：`🌟 {Region}` / `🔒 {Region}` / `🤯 {Region}`（候选来自所有 provider）
  - 落地组：`🛬 {base}`（按 `base-reality/base-ss` 与链式节点动态生成）与落地池 `🔒 落地`
- 落地组生成策略：
  - 若存在 `{base}-reality`：仅使用 `{base}-reality`（不使用 SS 及其链式）
  - 否则若存在 `{base}-ss`：优先 `{base}-JP|HK|KR`，并以 `{base}-ss` 兜底

### 6.3 缺失模板回退

- 若用户未配置 Mihomo profile，`format=mihomo` 回退到 `format=clash` 输出。
