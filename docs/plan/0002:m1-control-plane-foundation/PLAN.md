# Milestone 1 · 控制面基础（单机可用）（#0002）

## 状态

- Status: 已完成
- Created: 2025-12-17
- Last: 2025-12-17

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

- [x] **HTTP API Server**：Axum 路由组织、中间件、统一错误返回。
- [x] **认证**：管理员 Bearer token（仅覆盖管理员 API）。
- [x] **期望状态（单机）**：在本机保存并持久化 Nodes/Endpoints/Users/Grants，重启可恢复（最小持久化）。
- [x] **管理 API**：以 `docs/desgin/api.md` 为基准，补齐 Web 可用所需的读接口（列表/详情）。
- [x] **Web 联调**：确保现有 `web/` 能成功访问后端健康检查，并能在 UI 侧对管理 API 做请求验证（不要求完成完整面板功能）。

## 方案概述（Approach, high-level）

详见下方“原始输入”。

## 风险与开放问题（Risks & Open Questions）

- None noted in source.

## 参考（References）

- `docs/desgin/README.md`

## 原始输入（迁移前版本）

# Milestone 1 · 控制面基础（单机可用）— 需求与概要设计

> 对齐计划：`docs/plan/README.md` 的 **Milestone 1**。\
> 参考：`docs/desgin/requirements.md` / `docs/desgin/architecture.md` / `docs/desgin/api.md` / `docs/desgin/tech-selection.md`

## 1. 背景与目标

Milestone 1 的目标是把 `xp` 的“控制面骨架”在**单机**上跑通，形成可联调的最小闭环：

- `xp` 具备统一的工程约定（配置、日志、错误、ID）。
- 有可用的领域模型（Node / Endpoint / User / Grant）与校验规则。
- 提供管理员认证中间件：`Authorization: Bearer <admin_token>`。
- 提供管理 API（单机版）：对核心资源提供 CRUD（并为 future leader/follower 预留）。
- Web 联调：Vite proxy → `xp`；健康检查与最小页面贯通。

## 2. 范围与非目标

### 2.1 范围（M1 交付）

- **HTTP API Server**：Axum 路由组织、中间件、统一错误返回。
- **认证**：管理员 Bearer token（仅覆盖管理员 API）。
- **期望状态（单机）**：在本机保存并持久化 Nodes/Endpoints/Users/Grants，重启可恢复（最小持久化）。
- **管理 API**：以 `docs/desgin/api.md` 为基准，补齐 Web 可用所需的读接口（列表/详情）。
- **Web 联调**：确保现有 `web/` 能成功访问后端健康检查，并能在 UI 侧对管理 API 做请求验证（不要求完成完整面板功能）。

### 2.2 非目标（明确不做）

- Xray gRPC 适配、reconcile、运行态恢复（Milestone 2）。
- 订阅输出（Milestone 3）。
- 配额统计与封禁（Milestone 4）。
- Raft 集群、join/init、写转发（Milestone 5）。
- 完整 Web 面板（Milestone 6）。

## 3. 关键用例（单机）

1. 健康检查：Web 访问 `xp` 的健康接口并展示状态。
2. 管理员鉴权：未携带/错误 token 的请求被拒绝；正确 token 才能访问管理员 API。
3. 管理数据（期望状态）：
   - 创建/查看/删除 Endpoint；
   - 创建/查看/删除 User；支持 reset subscription token；
   - 创建/查看/删除 Grant；支持启用/禁用与修改配额字段（字段可先落模型，不实现配额逻辑）。
4. 集群信息占位：能查询 `GET /api/cluster/info`，单机模式下返回“本机为 leader”。

## 4. 数据/领域模型（M1）

> 目标：先把“形状”定住，方便后续 Raft 状态机复用。

### 4.1 通用约定

- 所有资源 ID 使用 **ULID 字符串**（见 `docs/desgin/api.md`）。
- 时间字段使用 RFC3339（带时区）。

### 4.2 实体与关系

- **Node**
  - `node_id: Ulid`
  - `node_name: string`（kebab-case，便于展示）
  - `public_domain: string`（订阅使用；M1 可先允许为空）
  - `api_base_url: string`（完整 origin，未来用于 leader 地址返回）
- **Endpoint**
  - `endpoint_id: Ulid`
  - `node_id: Ulid`（所属节点）
  - `kind: EndpointKind`（`vless_reality_vision_tcp` | `ss2022_2022_blake3_aes_128_gcm`）
  - `port: u16`
  - `tag: string`（全局唯一；建议 `"{kind-short}-{endpoint_id}"`）
  - `meta: EndpointMeta`（按 kind 分支；M1 只要求能保存/回读）
- **User**
  - `user_id: Ulid`
  - `display_name: string`
  - `subscription_token: string`（重置时生成新 token；M1 只存储与回传）
  - `cycle_policy_default: CyclePolicyDefault`（`by_user` | `by_node`）
  - `cycle_day_of_month_default: u8`（1–31）
- **Grant**
  - `grant_id: Ulid`
  - `user_id: Ulid`
  - `endpoint_id: Ulid`
  - `enabled: bool`（默认 true）
  - `quota_limit_bytes: u64`
  - `cycle_policy: CyclePolicy`（`inherit_user` | `by_user` | `by_node`）
  - `cycle_day_of_month: Option<u8>`（当 policy != inherit 时必填）
  - `note: Option<string>`
  - `credentials: GrantCredentials`（按 Endpoint.kind 生成；M1 可先生成并存储，但不要求下发到 xray）

关系：

- Node 1..n Endpoint
- User 1..n Grant
- Endpoint 1..n Grant

### 4.3 校验规则（M1 最低要求）

- `port`：1–65535
- `cycle_day_of_month*`：1–31
- `quota_limit_bytes`：允许为 0（表示无配额/禁用由后续策略定义；M1 不做语义约束）
- 引用完整性：创建 Grant 时 `user_id` 与 `endpoint_id` 必须存在

## 5. 接口与模块边界（M1）

### 5.1 HTTP 路由与前缀（与 Web 联调的约束）

统一约定：`xp` 对外所有 HTTP 路由使用 **`/api`** 作为固定前缀，便于 Web 与反代统一挂载：

- 健康检查：`/api/health`
- 管理员 API：`/api/admin/*`
- 集群内部：`/api/cluster/*`
- 订阅 API（后续）：`/api/sub/*`

> 当前 `web/` dev server 已代理 `/api`，因此 M1 不需要额外 rewrite 或多前缀 proxy。

### 5.2 管理 API（M1 最小集合）

以 `docs/desgin/api.md` 已定义的接口为准，M1 需要实现（或占位）：

- 集群信息（无鉴权）：`GET /api/cluster/info`
- Endpoints：
  - `POST /api/admin/endpoints`
  - `POST /api/admin/endpoints/{endpoint_id}/rotate-shortid`（仅对 vless 生效；M1 可先返回 501 或只更新期望状态）
- Users：
  - `POST /api/admin/users`
  - `POST /api/admin/users/{user_id}/reset-token`
- Grants：
  - `POST /api/admin/grants`
  - `PATCH /api/admin/grants/{grant_id}`
  - `GET /api/admin/grants/{grant_id}/usage`（M1 无用量来源，可先返回 501 或固定值并明确 `details.reason`）

为满足“CRUD + Web 可用”，建议在 M1 **补齐读接口**（接口形状建议与 create 返回保持一致）：

- `GET /api/admin/nodes`（单机返回 1 条）
- `GET /api/admin/endpoints` / `GET /api/admin/endpoints/{endpoint_id}`
- `GET /api/admin/users` / `GET /api/admin/users/{user_id}`
- `GET /api/admin/grants` / `GET /api/admin/grants/{grant_id}`

删除接口是否纳入 M1，取决于 Web 是否需要（建议先做）：

- `DELETE /api/admin/endpoints/{endpoint_id}`
- `DELETE /api/admin/users/{user_id}`
- `DELETE /api/admin/grants/{grant_id}`

### 5.3 认证与错误返回

- 管理员鉴权：对 `/api/admin/*` 统一使用中间件校验 Bearer token。
- 统一错误格式（见 `docs/desgin/api.md`）：
  - `error.code` 建议枚举：`unauthorized`、`forbidden`、`invalid_request`、`not_found`、`conflict`、`not_implemented`、`internal`
  - `error.details` 用于携带字段级错误、leader 地址（future）等。

### 5.4 Rust 模块划分（建议）

目标：后续引入 Raft / reconcile 时保持边界不变，减少返工。

- `config`：解析运行参数（bind、admin token、node info 等）。
- `domain`：Node/Endpoint/User/Grant 的结构、枚举、校验与序列化。
- `state`：期望状态存储接口（trait）+ 单机持久化实现（M1）。
- `service`：用例层（CRUD、token/credential 生成），只返回领域错误，不关心 HTTP。
- `http`：Axum 路由与 handler；把请求映射到 service，并把错误统一编码为 `api.md` 的错误格式。

## 6. 单机最小持久化（M1 必须）

目标：单机模式下，重启 `xp` 不丢失 Nodes/Endpoints/Users/Grants。

建议实现（最小快照，便于后续替换为 Raft）：

- 数据目录：通过配置指定（例如 `--data-dir` 或 `XP_DATA_DIR`），默认可为 `./data`。
- 快照文件：`state.json`（UTF-8 JSON），包含：
  - `schema_version`（整数）
  - `nodes/endpoints/users/grants` 的完整集合
- 写入策略：每次写请求成功后落盘：
  - 写入 `state.json.tmp`
  - `fsync`（可选，按平台能力）
  - 原子 `rename` 覆盖为 `state.json`
- 启动策略：
  - 若 `state.json` 存在则加载；
  - 若不存在则创建“单机默认 node”（从配置读取 `node_name/public_domain/api_base_url`，或使用合理默认）。

> M1 只要求“正确与可恢复”，性能优化（批量 flush、增量 WAL）留到 M5（Raft WAL）阶段统一处理。

## 7. 兼容性与迁移考虑

- **向 Raft 迁移**：M1 的 `state` 抽象应面向“期望状态”读写；未来替换为 Raft 状态机实现时，HTTP 层与 service 层尽量不改。
- **字段稳定性**：尽量避免在 M2+ 才新增“必填字段”；M1 先把核心字段定全（允许为空/默认值）。

## 8. 风险点与待确认问题

1. **管理员 token 来源**：M1 是要求从配置读取（ENV/CLI/文件），还是需要额外的 `xp init`（后续 M5）来生成并落盘？
2. **`/api/admin/grants/{id}/usage` 语义**：M1 无 Stats 来源时，是返回 501 更清晰，还是返回固定结构（`used_bytes=0`）以便前端先开发？
