# Milestone 3 · 订阅输出（对用户可交付）（#0004）

## 状态

- Status: 已完成
- Created: 2025-12-18
- Last: 2025-12-18

## 背景 / 问题陈述

本计划由旧 planning/spec 文档迁移归档；主人已确认该计划对应功能**已实现**。

## 目标 / 非目标

详见下方“原始输入”中的相关章节（例如“背景与目标”“范围与非目标”等）。

## 范围（Scope）

详见下方“原始输入”。

## 需求（Requirements）

详见下方“原始输入”。

## 接口契约（Interfaces & Contracts）

详见下方“原始输入”（本计划为迁移归档，不在此额外新增契约文档）。

## 验收标准（Acceptance Criteria）

详见下方“原始输入”中的 DoD/验收清单/验收点等章节（如有）。

## 里程碑（Milestones）

- [x] **订阅 API**：`GET /api/sub/{subscription_token}` 默认输出 Base64；支持 `?format=raw|clash`（见 `subscription.md`）。
- [x] **token 生命周期（只读边界清晰）**：
- [x] **订阅内容拼装**：

## 方案概述（Approach, high-level）

详见下方“原始输入”。

## 风险与开放问题（Risks & Open Questions）

- None noted in source.

## 参考（References）

- `docs/desgin/README.md`

## 原始输入（迁移前版本）

# Milestone 3 · 订阅输出（对用户可交付）— 需求与概要设计

> 对齐计划：`docs/plan/README.md` 的 **Milestone 3**。\
> 参考：`docs/desgin/subscription.md` / `docs/desgin/api.md` / `docs/desgin/architecture.md` / `docs/desgin/cluster.md`

## 1. 背景与目标

Milestone 1–2 已完成“期望状态 + 本机 xray 运行态收敛”。但对终端用户而言仍缺少最关键的交付物：**可直接导入客户端的订阅**。

Milestone 3 的目标是提供稳定的订阅输出能力，使“创建端点 → 创建用户/授权 → 拉取订阅 → 客户端连通”在单机环境闭环可交付。

## 2. 范围与非目标

### 2.1 范围（M3 交付）

- **订阅 API**：`GET /api/sub/{subscription_token}` 默认输出 Base64；支持 `?format=raw|clash`（见 `subscription.md`）。
- **token 生命周期（只读边界清晰）**：
  - token 在创建 User 时生成；
  - 支持管理员重置 token（旧 token 立即失效）；
  - 订阅接口只读，不需要管理员鉴权。
- **订阅内容拼装**：
  - `host` 固定使用 Endpoint 所属节点的 `Node.public_domain`；
  - `port` 使用 `Endpoint.port`；
  - 输出内容与 Web 展示的连接信息一致（同一份源数据）。

### 2.2 非目标（明确不做）

- 订阅 token 的“定时过期/吊销列表/分级权限”（MVP 不引入复杂生命周期）。
- 配额与超限封禁（Milestone 4）。
- Raft 集群一致性、写转发（Milestone 5）。
- Web 面板中的订阅 UI（Milestone 6 才做“一键复制/格式切换”等体验）。

## 3. 关键用例 / 用户流程

1. 管理员创建 User 与 Grants。
2. 用户拿到 `subscription_token`（或由管理员代取）。
3. 用户访问：
   - `GET /api/sub/{token}` → Base64（默认）
   - `GET /api/sub/{token}?format=raw` → Raw URI（逐行）
   - `GET /api/sub/{token}?format=clash` → Clash YAML
4. 管理员重置 token：旧订阅链接立即不可用，新链接可用。

## 4. 数据与领域模型（M3）

M3 不新增持久化实体，订阅输出完全由既有“期望状态”派生：

- `User`：`subscription_token`、`display_name`、周期默认值（订阅输出只需要前两者）。
- `Grant`：`enabled`、`note`、`credentials`、`endpoint_id`。
- `Endpoint`：`kind`、`port`、`tag`、`meta`。
- `Node`：`public_domain`、`node_name`。

派生概念（非持久化）：

- **SubscriptionItem**：`{name, protocol, host, port, payload...}`（最终可编码为 raw/clash）。

校验与错误策略（最小但清晰）：

- token 不存在：返回 404（避免“可枚举”的行为差异）。
- 订阅需要的字段缺失（如 `Node.public_domain` 为空、VLESS `server_names` 为空等）：返回错误（见 §5.3）。

## 5. 接口设计

### 5.1 路由

- `GET /api/sub/{subscription_token}`
- Query：
  - `format=raw`：输出 Raw URI（逐行）
  - `format=clash`：输出 Clash YAML
  - 省略或其它值：默认输出 Base64（若为未知值则视为 invalid_request）

### 5.2 返回内容类型

- `format=raw`：`text/plain; charset=utf-8`
- 默认 Base64：`text/plain; charset=utf-8`
- `format=clash`：`text/yaml; charset=utf-8`

### 5.3 错误返回（建议）

当发生错误时仍返回统一 JSON 错误格式（`docs/desgin/api.md`），例如：

- 404：`not_found`（token 不存在）
- 400：`invalid_request`（format 非法）
- 500：`internal`（数据不满足订阅输出的必要条件，或内部构建失败）

> 说明：订阅成功响应是纯文本/YAML；错误响应用 JSON 便于定位问题与自动化测试。

## 6. 输出规格（与 subscription.md 对齐）

### 6.1 NAME 规则

- 默认：`{user.display_name}-{node.node_name}-{endpoint.tag}`
- 若 `Grant.note` 非空：用 `Grant.note` 覆盖

实现建议：

- 对 URI 的 `#<NAME>` 做最小 URL encode（避免空格/特殊字符导致解析差异）。

### 6.2 Raw URI（逐行）

#### 6.2.1 VLESS + REALITY（vision/tcp）

按 `docs/desgin/subscription.md` 模板拼装：

`vless://<UUID>@<HOST>:<PORT>?encryption=none&security=reality&type=tcp&sni=<SNI>&fp=<FP>&pbk=<PBK>&sid=<SID>&flow=xtls-rprx-vision#<NAME>`

字段来源：

- `UUID`：`Grant.credentials.vless.uuid`
- `HOST`：`Node.public_domain`
- `PORT`：`Endpoint.port`
- `SNI`：`Endpoint.meta.reality.server_names[0]`
- `FP`：`Endpoint.meta.reality.fingerprint`
- `PBK`：`Endpoint.meta.reality_keys.public_key`
- `SID`：`Endpoint.meta.active_short_id`

#### 6.2.2 Shadowsocks 2022（tcp+udp）

按 `docs/desgin/subscription.md` 模板拼装：

`ss://2022-blake3-aes-128-gcm:<PASSWORD>@<HOST>:<PORT>#<NAME>`

字段来源：

- `method`：固定 `2022-blake3-aes-128-gcm`
- `password`：`Grant.credentials.ss2022.password`（形如 `<server_psk_b64>:<user_psk_b64>`）
- `PASSWORD`：对 `password` 进行 percent-encoding 后的字符串（SIP002 对 AEAD-2022 的约束）

### 6.3 Base64 订阅（默认输出）

规则：

- 以 Raw URI 的完整文本（含换行）作为输入（UTF-8）
- 对整体做 RFC4648 base64 编码
- 输出不换行

### 6.4 Clash YAML（Mihomo/Clash.Meta）

MVP 输出最小可导入 YAML：

- 仅包含 `proxies: [...]`
- 不额外生成 `proxy-groups` 与 `rules`（避免替用户做过多假设）

字段映射见 `docs/desgin/subscription.md` 的示例。

## 7. 模块边界与实现建议（M3）

建议新增（或内聚在 `http` 旁的）订阅生成模块，避免把拼装细节塞进 handler：

- `subscription`：订阅派生与编码
  - `build_raw_lines(user, grants, endpoints, nodes) -> Vec<String>`
  - `encode_base64(raw_text) -> String`
  - `render_clash_yaml(items) -> String`
- `state`：增加按 token 查询用户的便捷方法（或建立索引，后续迁移到 Raft 时再抽象）
- `http`：只做参数解析、数据读取与响应编码

## 8. 兼容性与迁移考虑

- **向后兼容**：M1 已生成的 `subscription_token` 继续有效；重置 token 会立即切换到新 token。
- **向 M5（Raft）迁移**：订阅输出的输入应来自“已复制的期望状态”；未来可在状态机层提供 `token -> user_id` 的索引，避免全表扫描。

## 9. 测试计划（M3）

- 单测/接口测试：
  - token 不存在返回 404
  - `format` 非法返回 400
  - VLESS/SS 各生成一条授权，`format=raw` 输出行数与内容正确
  - 默认 Base64 能解码回 Raw 文本（逐行一致）
  - `format=clash` 输出 YAML 可被解析，字段存在且与 raw 对齐
- 回归：确保不影响现有管理员 API 与 reconcile 行为。

## 10. 验收清单（M3 DoD）

- `GET /api/sub/{token}` 默认返回 Base64，且与 `format=raw` 编码规则一致。
- `GET /api/sub/{token}?format=raw` 返回逐行 Raw URI（VLESS/SS2022 正确拼装）。
- `GET /api/sub/{token}?format=clash` 返回最小 Clash YAML（proxies 可导入）。
- `reset-token` 后旧 token 立即失效，新 token 可用。
- 订阅输出使用 `Node.public_domain` 与 `Endpoint.port`，并与 Web 展示一致。

## 11. 风险点与已定策略

1. **SS2022 URI 兼容性**：采用 SIP002 的 plain userinfo（AEAD-2022 不做 Base64URL userinfo），并对 `password` 做 percent-encoding；需用测试向量覆盖 `:`、`+`、`/`、`=` 等字符。
2. **NAME 编码**：对 URI 的 `#<NAME>` 做 URL encode（已定），避免空格/特殊字符导致解析差异。
3. **输出范围**：仅输出 `enabled=true` 的 Grants（已定），避免“订阅里存在但实际连不上”的配置。
4. **public_domain 必填**：`Node.public_domain` 为空时返回错误（已定），避免生成不可用订阅。
