# Grant 模型：分组与配额周期归属（#0017）

## 状态

- Status: 待实现
- Created: 2026-01-18
- Last: 2026-01-19

## 1) 问题陈述

当前系统以 `Grant(user_id, endpoint_id)` 作为主要配置单元，且 `Grant` 上携带配额与周期字段（`quota_limit_bytes`、`cycle_policy`、`cycle_day_of_month`）。与此同时，管理端最新交互形态倾向于：

- 列表视角：以“分组（group）”为主展示单位，而不是逐条 grant；
- 操作视角：对多维对象做操作（例如多用户/多节点/多协议），并映射到持久化的一维结构。

这使得现有的 grant 数据格式与 UI 语义出现偏差，且“配额重置时间/周期”作为 grant 自身字段也不符合预期归属：grant 不应承载该配置；重置配置应存在于 **User/Node**，并支持在“节点行”粒度选择参考用户配置或节点配置（默认参考用户配置）。

本计划用于冻结：

- “分组（group）”在数据模型与 API 中的形状；
- “配额周期/重置时间”的归属与口径；
- UI 的多维操作如何落到持久化模型（以及兼容/迁移策略）。

## 2) 目标 / 非目标

### Goals

- 定义并冻结一个与最新 UI 语义一致的 grant 数据模型（含分组能力）。
- 明确并冻结“配额周期/重置时间”的归属：grant 不承载；User/Node 均可配置；节点行可选择参考 user 或 node（默认 user），并给出可实现、可测试的契约。
- 给出从现有持久化与 API 口径迁移到新口径的策略（含回滚考虑）。

### Non-goals

- 不在本计划阶段改动实现代码/迁移/依赖（仅冻结口径与契约）。
- 不在本计划内设计完整 UI 细节与视觉稿（UI 交互另由相关计划推进）。

## 3) 用户与场景

- **用户**：控制面管理员 / 运营人员。
- **场景**
  - 在管理端以“分组”为主视角查看与管理授权，而不是被大量 grant 记录淹没。
  - 一次性对多维对象执行操作（多用户/多节点/多协议），并落盘为一致、可追溯的数据结构。
  - 配额周期/重置时间在系统中有唯一归属，避免同一批授权出现多套周期口径。

## 4) 需求列表（MUST）

### MUST

- 数据模型必须支持“分组视角”：
  - 每条 grant（或等价的授权记录）必须可归属到一个 `group`（用于 UI 分组展示与操作聚合）。
  - `group` 的标识采用 **A1（name-as-key）**：`group_name` 全局唯一（并作为 UI 展示名）。
- 分组必须支持“整组提交（submit whole group）”与“组改名（rename）”：
  - 管理端不做“逐条 grant 的交互式操作”；提交必须以“组”为单位提交变更。
  - 组改名必须是原子语义（要么成功更新，要么失败不写入）。
  - 不得存在 `grants API` 作为前后端交互入口；管理端前后端只使用 `grant-groups` 进行读写。
  - 默认不允许“空分组”：创建/更新需要至少 1 个成员；删除分组使用 group-level delete。
- 多维操作必须能落到持久化结构：
  - 必须明确定义“UI 的多维选择”如何映射成持久化写入（新增/删除/更新）。
  - 必须定义去重规则：**不允许**同一 `(user_id, endpoint_id)` 出现在多个 group（冲突返回 `409`）。
- **配额周期/重置时间不得归属在 grant**：
  - `Grant`（或等价授权记录）不得再持有周期字段（`cycle_*`）作为源数据；
  - User 与 Node 均可配置“流量重置（按月|无限）”与时区；
  - 在 grant 配置 UI（节点行）上，每个节点可选择“参考用户配置”或“参考节点配置”（默认参考用户配置）。
- 兼容与迁移必须明确：
  - 必须给出旧数据（持久化与 API）向新口径迁移的规则（字段默认值、缺失值处理、冲突处理）。
  - 必须提供可测试的迁移验收（至少覆盖：旧格式可加载 → 新格式写回）。

## 5) 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）          | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）    | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                         |
| --------------------- | ------------ | ------------- | -------------- | --------------------------- | --------------- | ------------------- | ------------------------------------- |
| AdminGrantGroups      | HTTP API     | internal      | New            | ./contracts/http-apis.md    | backend         | web                 | group-level 提交/改名/删除与列表/详情 |
| AdminGrants（Legacy） | HTTP API     | internal      | Delete         | ./contracts/http-apis.md    | backend         | web                 | 移除 `/api/admin/grants*` 系列接口    |
| AdminUsers            | HTTP API     | internal      | Modify         | ./contracts/http-apis.md    | backend         | web                 | 用户流量重置配置（按月                |
| AdminNodes            | HTTP API     | internal      | Modify         | ./contracts/http-apis.md    | backend         | web                 | 节点流量重置配置（按月                |
| AdminUserNodeQuotas   | HTTP API     | internal      | Modify         | ./contracts/http-apis.md    | backend         | web                 | 节点行选择参考 user/node（默认 user） |
| PersistedState        | File format  | internal      | Modify         | ./contracts/file-formats.md | backend         | ops                 | `state.json` schema_version 迁移      |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/file-formats.md](./contracts/file-formats.md)

### UI 设计

- [DESIGN.md](./DESIGN.md)

## 6) 约束与风险

- 约束：当前 `state.json` 有 `schema_version` 校验；任何结构变更都需要迁移策略与版本递增。
- 风险：A1（name-as-key）下的组改名需要批量重写 N 条 grants；必须由服务端单事务/单 raft 命令保障原子与性能上限。
- 风险：流量重置配置从 grant 上移除后，quota 计算、usage 展示、封禁/解封逻辑会受影响；必须确保“默认值与历史语义一致”（含时区与短月处理）。

## 7) 验收标准（Acceptance Criteria）

- Given 管理端需要按分组展示授权与进行批量操作，
  When 拉取 grant groups（或等价数据）并按分组聚合，
  Then 每条授权均能映射到一个稳定的 `group`，且聚合结果稳定一致。

- Given 系统存在旧格式持久化数据（含 grant 上的 `cycle_*` 字段），
  When 服务启动加载并完成迁移，
  Then 新格式可被写回且再次启动不会报 `schema_version mismatch`。

- Given User 与 Node 均定义了“流量重置配置（按月|无限）”且 grant 不承载，
  When 管理端查看/设置重置相关字段并在节点行选择参考 user 或 node，
  Then 字段来源明确、校验一致、默认值正确（User 默认 UTC+8；Node 默认服务器时区），且 grant 不再承载重置源数据。

## 8) 非功能性验收 / 质量门槛（Quality Gates）

实现阶段完成后，至少运行（沿用仓库现有约定，不引入新工具）：

- Backend: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`
- Web: `cd web && bun run lint`, `cd web && bun run typecheck`, `cd web && bun run test`

### Testing（需要补齐的测试）

- 后端：`schema_version` 迁移测试（旧格式 → 新格式），覆盖至少 1 个 grant 与其分组字段、周期字段迁移。
- 后端：新增/修改 API schema 的序列化/反序列化与校验测试。
- 前端：最小化的 schema 解析（zod）与聚合逻辑单元测试（如引入 group 视角）。

## 9) 文档更新（Docs to Update）

- `docs/plan/0014:grant-node-quota-editor/PLAN.md`: 如周期归属/配额归属口径变化，需要同步对齐（避免实现时冲突）。
- `docs/plan/0016:grant-new-access-selection/PLAN.md`: 如“分组/去重/批量操作”口径影响创建流程，需要同步对齐。
- `docs/desgin/api.md`: 将 `Grants` 章节替换为 `Grant groups`（并同步 user/node reset 配置与 `node-quotas` 字段）。
- `docs/desgin/quota.md`: 同步“按月|无限 + 时区 + 短月处理”的口径与示例。

## 10) 里程碑（Milestones）

- [ ] M1: Backend 落地 schema v2（含迁移）与 grant-groups APIs（删除 grants APIs）
- [ ] M2: Web 适配 grant-groups 交互（创建/列表/编辑以“组”为单位）
- [ ] M3: 补齐测试与质量门槛（迁移测试 + web typecheck/lint/test）

## 11) 方案概述（Approach, high-level）

- 以“分组”为 UI 的一等展示单位：采用 A1（`group_name` 作为 identity）与 group-level replace API 来支撑整组提交与原子改名。
- 将重置/周期字段从 grant 上移除：User/Node 均可配置；节点行可选择参考 user 或 node（默认 user）。
- 通过 `schema_version` 迁移确保老数据可加载并稳定写回新格式。

### 分组存储方案对比（已定案：A1）

| 方案                                        | 数据落盘形态                                                                                          | 优点                                                                            | 代价/风险                                                                                                         | 适配度                                                 |
| ------------------------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| A1. `Grant.group_name`（name-as-key）       | grants 仍是一等记录；`group_name` 作为分组唯一标识（同时 UI 展示名），组改名=服务端批量重写           | 迁移/实现成本最低；与“整组提交”天然契合；无需新增 group 元数据表/Map            | 改名为 O(N) 重写；需要冻结 `group_name` 归一化/字符集/长度；需要服务端事务/raft 保证原子                          | **本计划已定案采用**                                   |
| B. 多维存储（Group 为一等实体 + 规则/成员） | 落盘为 `GrantGroup{members/selectors}`；运行时需要“物化（materialize）”成授权集合，或在读写时动态计算 | 分组语义强；可天然表达多用户/多节点/多协议；组级别操作更原子（改 1 处影响整组） | 复杂度高：需要新增持久化结构/命令/API/迁移；需要设计“物化策略、幂等、冲突、审计、回滚”；可能牵动订阅凭据/历史兼容 | 更贴近“支持多用户、多节点、多协议的存储”，但实现面更大 |

说明：若未来明确需要“动态选择器/规则”（例如按标签选择节点、组内排除某些组合），再评估演进到 **B**（不属于本计划交付范围）。

> 注：本计划已收到硬约束“必须支持组改名 + 前端只做整组提交 + 不得存在 grants API”。这会强烈倾向于：以 `grant-groups` 作为唯一交互面，并提供服务端的 group-level replace API（单次写入完成改名与成员变更），以保证原子语义与幂等性。

## 12) 风险与开放问题（Risks & Open Questions）

- 风险：
  - A1 方案下组改名/整组变更是 O(N) 批量重写；实现阶段需要通过事务/raft 确保原子与幂等，并对大 N 提供明确反馈。
  - 重置配置从 grant 上移除会牵动 quota tick 与 usage 展示；需要明确时区、短月与默认值边界条件。

开放问题：
None（关键口径已定案）

## 13) 假设（已确认）

- 假设：分组采用 A1（`group_name` 作为 identity，且唯一）；整组提交通过 group-level replace/rename API 原子落地。
- 假设：同一 `(user_id, endpoint_id)` 全局唯一（不允许出现在多个 group）。
- 假设：`group_name` 采用 slug 并限制长度（实现阶段双端校验）：
  - regex: `[a-z0-9][a-z0-9-_]*`
  - length: `1..=64`
- 假设：User/Node 的时区口径：
  - User 默认 `tz_offset_minutes=480`（UTC+8），可配置。
  - Node 默认 `tz_offset_minutes=null`（服务器时区），可配置。
- 假设：月度重置发生在所选时区的“当日 00:00”；当 `day_of_month=31` 遇到短月时落到当月最后一天（与现有 cycle 口径一致）。
- 假设：`policy: "unlimited"` 语义为“无限流量/不做配额封禁”，且 User/Node 均支持该选项。
- 假设：节点行的“参考 user/node”默认值为 `user`。

## 参考（References）

- 现状类型：`src/domain/mod.rs`（`Grant`、`User`、`Node`）
- 周期计算：`src/cycle.rs`
- Legacy admin grants API（待删除）：`src/http/mod.rs`（`/api/admin/grants`）
- Admin grant groups API（新增）：`src/http/mod.rs`（`/api/admin/grant-groups`）
