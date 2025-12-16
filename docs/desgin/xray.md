# xp · Xray 集成说明（API / 动态入站 / 统计）

本文件定义 `xp` 与本机 `xray` 的交互方式，以及 `xray` 必须满足的基础配置约束。

## 1. 配置格式与基本原则

- Xray 配置文件为 **JSON**（服务端与客户端格式一致，仅内容不同）。
- 本项目采用：**基础配置静态 + 入站/用户动态注入** 的方式：
  - 静态：log、outbounds、api、stats、policy（以及必要的 routing）；
  - 动态：所有业务入站（Endpoints）与 clients（Grants）。

> 原因：你要求“动态管理每个入站的客户端”，且确认直接使用 HandlerService 的 AddInbound 能力。

## 2. Xray 必须启用的能力

### 2.1 gRPC API（HandlerService + StatsService）

Xray 通过 `api` 模块启用 gRPC API。关键点：

- 必须启用 `HandlerService`（动态入站与用户增删）
- 必须启用 `StatsService`（读取 uplink/downlink 统计）
- `api.listen` 可直接指定监听地址（若省略，需要自行配置 api inbound + routing 到 `outboundTag=api`）

### 2.2 流量统计（stats + policy）

要得到“按用户（client/email）”统计，必须满足：

- 配置中存在 `stats: {}`
- `policy.levels.<level>.statsUserUplink=true` 且 `statsUserDownlink=true`
- 每个 client 必须设置 `email`（否则无法按用户区分统计）

`xp` 约定：所有 Grant 的 client email 固定为 `grant:<grant_id>`。

## 3. 推荐的 Xray 基础配置（示例）

> 说明：这是 **基础配置**，不包含业务入站。业务入站由 `xp` 通过 AddInbound 动态下发。

```json
{
  "log": { "loglevel": "warning" },
  "api": {
    "tag": "api",
    "listen": "127.0.0.1:10085",
    "services": ["HandlerService", "StatsService"]
  },
  "stats": {},
  "policy": {
    "levels": {
      "0": { "statsUserUplink": true, "statsUserDownlink": true }
    }
  },
  "inbounds": [],
  "outbounds": [
    { "tag": "direct", "protocol": "freedom", "settings": {} },
    { "tag": "block", "protocol": "blackhole", "settings": {} }
  ]
}
```

> 如果你更偏向“完全兼容旧版本配置方式”，可以不用 `api.listen`，改为手动添加 `dokodemo-door` api inbound + routing 规则。`xp` 实现阶段会同时支持两种生成模式（默认使用 `api.listen`）。

## 4. 动态下发：Endpoint 与 Grant 如何落到 Xray

### 4.1 Endpoint → AddInbound / RemoveInbound

- 创建 Endpoint：`HandlerService.AddInbound(tag, inboundConfig)`
- 删除 Endpoint：`HandlerService.RemoveInbound(tag)`

> 由于 Xray 动态入站不持久：`xray` 重启后入站会丢失，因此 `xp` 启动必须 reconcile（按期望状态重建所有 Endpoint 并重放 Grants）。

### 4.2 Grant → AlterInbound(AddUser / RemoveUser)

- 启用 Grant：对对应 inbound 执行 `AddUserOperation`
- 禁用 Grant：对对应 inbound 执行 `RemoveUserOperation`

约束：

- VLESS / Trojan / VMess / Shadowsocks(v1.3.0+) 才支持 API 动态增删用户。
- 本项目只用：VLESS 与 Shadowsocks（包含 SS2022）。

## 5. 统计读取：配额的“数据源”

`xp` 使用 StatsService 周期性读取：

- `user>>>grant:<grant_id>>>traffic>>>uplink`
- `user>>>grant:<grant_id>>>traffic>>>downlink`

并以 `uplink + downlink` 作为 Grant 的累计使用量（双向合计）。

## 6. REALITY 的 shortId 与 publicKey（落地约束）

### 6.1 shortId

- server 端：`realitySettings.shortIds` 是可接受 shortId 列表
- client 端：订阅里用 `sid=<shortId>`
- shortId 约束：
  - 十六进制字符串
  - 长度为 2 的倍数
  - 最长 16 个字符
  - 若服务端允许列表包含空串，则客户端可留空（本项目禁止空串）

### 6.2 publicKey（pbk）

- 订阅中 `pbk` 为服务端 REALITY 私钥对应的公钥（x25519）
- 运维上可用 `xray x25519 -i "<privateKey>"` 验证推导关系
