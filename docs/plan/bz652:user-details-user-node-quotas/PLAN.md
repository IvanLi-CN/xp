# User 详情：用户信息 / 接入点配置（矩阵）（#bz652）

## 状态

- Status: 待实现
- Created: 2026-01-30
- Last: 2026-01-30

## 背景 / 问题陈述

- 需要替换 `UserDetailsPage` 的主内容为清晰的两个标签页：`User`（用户信息）与 `Node quotas`（矩阵型接入点配置）。
- 接入点配置必须是“矩阵型”（节点 × 协议），对齐现有 `GrantNewPage` 的交互口径（`GrantAccessMatrix`）。
- UI 不应暴露底层 Grant 表字段与 group 口径；group 仅作为实现细节，用于“本用户配置”的落库与硬切。

## 目标 / 非目标

### Goals

- User 详情页仅包含两个 tabs：`User` 与 `Node quotas`（矩阵）。
- `User` tab：展示并编辑用户信息（沿用当前 `UserDetailsPage` 能力）。
- `Node quotas` tab：矩阵型配置该用户的接入点（nodes × protocols），并支持“硬切（hard cut）”保存。
- 视觉与交互风格对齐现有 `web/src/views/GrantNewPage.tsx` 的布局密度与组件形态（不引入新的页面风格）。

### Non-goals

- 不在 UI 中新增/暴露底层 Grant 模型字段（例如 `grant_id`、cycle 字段、group 概念）。
- 不在本计划内重做 endpoints CRUD 页面（`/endpoints/*`）。

## 范围（Scope）

### In scope

- Web Admin UI：替换 `web/src/views/UserDetailsPage.tsx` 的主内容结构，改为 tabs：`User` / `Node quotas`（见 UI 草图）。
- `Node quotas` tab：
  - 矩阵（nodes × protocols）展示可用接入点；单元格可 on/off；若同一 node+protocol 有多个 endpoint，支持在 cell 内选择具体 endpoint（与 `GrantAccessMatrix` 一致）。
  - 在矩阵行（node）处编辑该用户在该节点的流量配额（`quota_limit_bytes`），使用 `MiB/GiB` 的用户友好输入形状（复用既有 `NodeQuotaEditor` 口径）。
  - 保存口径为“硬切”：以当前选择覆盖“本用户 managed group”的 members。
  - 允许“全关闭”：当选择为空时，删除本用户 managed group（等价于该用户无接入点）。

### Out of scope

- 后端数据迁移/清洗与回填策略（除非实现阶段发现必须）。
- 任意跨用户批量编辑能力。

## 需求（Requirements）

### MUST

- User 详情页只展示两个 tabs：`User` / `Node quotas`（矩阵）。
- `User` tab：
  - 复用现有 UserDetails 的用户信息编辑能力（display name、quota reset 等），保留订阅预览 / Reset token 与删除用户等入口（不增加新字段）。
- `Node quotas` tab：
  - UI 不出现 group/Grant 表字段（例如 `grant_id`、cycle 字段）。
  - 矩阵型接入点配置（节点 × 协议），不允许出现“列表式的接入点配置”替代矩阵。
  - 必须支持修改该用户在每个节点的流量配额（`quota_limit_bytes`），并保持最终生效配额与 UI 回显一致。
  - 保存口径为“硬切”：
    - 保存后刷新回显与矩阵选择一致；
    - 允许空选择（删除 managed group）。
- 风格对齐：卡片结构、工具条密度、按钮样式与 `web/src/views/GrantNewPage.tsx` 保持一致观感。

## 接口清单与契约（Interfaces & Contracts）

| 接口（Name）                              | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）   | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                                                 |
| ----------------------------------------- | ------------ | ------------- | -------------- | -------------------------- | --------------- | ------------------- | ------------------------------------------------------------- |
| UserDetails tabs (`User` / `Node quotas`) | UI Component | internal      | Modify         | `./contracts/ui.md`        | web             | admin               | 仅重组布局与信息架构                                          |
| User access matrix APIs                   | HTTP API     | internal      | Modify         | `./contracts/http-apis.md` | backend         | web/admin           | 复用既有 endpoints / grant-groups；本计划新增“明确的使用契约” |

## 验收标准（Acceptance Criteria）

- Given 我进入某个用户详情页，
  When 我查看页面 tabs，
  Then 我只看到 `User` 与 `Node quotas` 两个标签页。
- Given 我在 `User` tab 修改 display name 并保存，
  When 后端返回成功，
  Then 页面提示成功，刷新后展示新值。
- Given 我进入 `Node quotas` tab，
  When 页面加载完成，
  Then 我能看到“节点 × 协议”的接入点矩阵，且页面不展示任何 Grant 表字段与 group 字段。
- Given 我在 `Node quotas` tab 修改某个节点的 Quota 并确认保存，
  When 操作成功并刷新回显，
  Then 该节点行显示新的 quota 值，且该用户在该节点上已选 endpoint 的实际生效 `quota_limit_bytes` 与该值一致。
- Given 我在 `Node quotas` tab 勾选/取消若干 cell 并点击 `Apply changes`，
  When 操作成功并刷新，
  Then 矩阵回显与我刚刚的选择一致。
- Given 我把所有 cell 都关闭（空选择）并点击 `Apply changes`，
  When 操作成功并刷新，
  Then 矩阵显示为全关闭（该用户无接入点），且不会报 “members must have at least 1 member” 一类错误。

## 实现前置条件（Definition of Ready / Preconditions）

- UI 草图已由主人确认（见下方 References）。
- 本用户 managed group 命名规则（由本计划冻结）：
  - `group_name = managed-${sanitize_group_name_fragment(user_id)}`（实现侧可复用 `src/state.rs` 的同名逻辑）
  - 约束：最长 64；仅 `[a-z0-9-_]`；首字符必须为 `[a-z0-9]`
- 确认 `UserDetailsPage` Header 里的 “New grant group” 入口移除（计划默认移除以满足“本页只做本用户配置”）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Web unit tests（Vitest）：覆盖 tabs 切换与 Node quotas matrix 的 apply→回显流程（至少 1 个节点 + 1 个协议）。

### UI / Storybook (if applicable)

- 为 User details 新 tabs 补充 story（如仓库现有模式允许）。

### Quality checks

- Frontend：`cd web && bun run lint && bun run typecheck && bun run test`

## 文档更新（Docs to Update）

- None（若后续确认需要更新 `docs/desgin/*`，再补充到此处）。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

TBD（在 UI 草图确认并满足 `Ready-to-implement: yes` 后再拆分；避免在待设计阶段提前排期）。

## 方案概述（Approach, high-level）

- `User` tab 保留现有用户编辑/订阅/危险操作逻辑（仅移动位置/结构，不改语义）。
- `Node quotas` tab：
  - 复用 `GrantAccessMatrix` 的交互口径（节点 × 协议）。
  - 在 node 行内复用 `NodeQuotaEditor` 的交互与解析口径（MiB/GiB），并通过 `PUT /api/admin/users/:user_id/node-quotas/:node_id` 写入用户级配额。
  - 若 managed group 已存在且该节点有已选 endpoint，则同步用 `PUT /api/admin/grant-groups/:group_name` 硬切更新对应 members 的 `quota_limit_bytes`，确保“实际生效配额”与 UI 一致。
  - 以“本用户 managed group”作为落库与回显来源：
    - 加载：`GET /api/admin/grant-groups/:group_name`（404 视为空）。
    - 保存：
      - 若选择为空：`DELETE /api/admin/grant-groups/:group_name`
      - 若选择非空：`POST /api/admin/grant-groups`（首次）或 `PUT /api/admin/grant-groups/:group_name`（后续硬切）

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：managed group 的命名若不够短/不满足校验，会导致无法创建或碰撞；需要在实现前冻结规则。
- 风险：后端 `PUT /grant-groups/:group_name` 不允许空 members；因此 UI 必须显式走 DELETE 路径处理空选择（本计划已冻结该口径）。

## 变更记录（Change log）

- 2026-01-30: pivot to matrix-based access configuration per user (replace list UI)
- 2026-01-30: align sketches to current UI (GrantNewPage)
- 2026-01-30: add per-node quota editing in Node quotas tab (sync to managed group)

## 参考（References）

- 相关页面：
  - `web/src/views/UserDetailsPage.tsx`
  - `web/src/views/GrantNewPage.tsx`
- UI 草图（plan-only，已修复头部 badges 溢出）：
  - [sketches/user-details-user-tab.svg](./sketches/user-details-user-tab.svg)
  - [sketches/user-details-node-quotas-tab.svg](./sketches/user-details-node-quotas-tab.svg)
