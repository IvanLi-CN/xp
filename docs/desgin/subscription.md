# xp · 订阅输出规格（URI / Base64 / Clash YAML）

## 1. 输出入口

- `GET /api/sub/{subscription_token}`：默认返回 Base64（便于大多数客户端直接导入）
- `GET /api/sub/{subscription_token}?format=raw`：返回纯 URI（逐行）
- `GET /api/sub/{subscription_token}?format=clash`：返回 Clash YAML（Mihomo/Clash.Meta）

## 2. 统一规则

### 2.1 host 与端口

- `host`：使用 Endpoint 所属节点的 `Node.access_host`
- `port`：使用 Endpoint 的入站端口

### 2.2 命名（显示名）

- URI 的 `#name` 与 Clash 的 `name`：
  - 默认：`{user.display_name}-{node.name}-{endpoint.tag}`
  - 可被 Grant.note 覆盖（更友好）
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
