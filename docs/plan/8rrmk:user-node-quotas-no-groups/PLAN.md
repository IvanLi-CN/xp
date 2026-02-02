# 用户接入点配置：不再依赖 Grant groups（移除 managed group 假设）（#8rrmk）

## 状态

- Status: 待实现
- Created: 2026-02-02
- Last: 2026-02-02

## 背景 / 问题陈述

- 当前 Web UI `User -> Node quotas` 仅读取/写入一个“本用户 managed group”（`managed-<user_id>`）来表达用户接入点配置。
- 这会导致明显错觉：当矩阵里**一个接入点都没勾选**时，用户订阅仍可能输出多条（因为用户仍可能通过其它 grant/group 获取到 endpoints）。
- 现实需求：我们不再使用“组（grant group）”作为用户接入点配置的核心概念；`Node quotas` 应反映与管理用户**实际生效的接入点集合**，而不是某个特定 group 的子集。

## 目标 / 非目标

### Goals

- Web UI `Node quotas` 展示的是用户**实际生效**的接入点（enabled grants）：
  - 即使 grant 来源于历史 group，也应纳入展示（避免“全空但订阅有内容”的困惑）。
- Web UI `Apply changes` 采用“硬切（hard cut）”语义：保存后用户实际生效的接入点集合 == UI 选择集合。
- 彻底移除 `Node quotas` 对 `managed-<user_id>` / `grant-groups` 的依赖（UI/接口层面不再出现“组”的概念）。
- 保持订阅输出与 UI 一致：当用户未选择任何接入点时，订阅输出应为空；选择 N 个时输出 N 条。

### Non-goals

- 不在本计划内彻底移除系统中所有 `grant-groups` 相关能力（可作为后续计划）。
- 不在本计划内重做订阅格式/排序（除非为验收必须）。

## 范围（Scope）

### In scope

- 后端新增“按 user 管理 grants（无 group 概念）”的 Admin API（见 `contracts/http-apis.md`）：
  - 读取：列出用户当前实际生效的 grants（跨历史 group/空 group）
  - 写入：以 hard cut 方式替换用户 grants（不再通过 grant-group replace）
- Web UI `UserDetailsPage -> Node quotas`：
  - 读取使用新 API 构建矩阵选择状态
  - 保存使用新 API 执行 hard cut
  - 不再读取/写入 `managed-<user_id>` 的 grant group
- 数据迁移/兼容：
  - 读取时兼容历史 grants（带 `group_name` 的存量）
  - 写入 hard cut 时将结果落为“无 group”的 grants（`group_name=""`），并清理/禁用该用户的历史 grants（以实现“无隐藏接入点”）
- 测试：
  - 后端 HTTP tests 覆盖：无选择 -> 0 条订阅；有选择 -> N 条；hard cut 覆盖历史 group grants
  - Web tests 覆盖：Node quotas 初始态与订阅一致；Apply changes 后一致

### Out of scope

- 前端移除“Grant groups”页面/导航入口（可后续讨论）。
- 对外（非 admin）API 的任何破坏性变更。

## 需求（Requirements）

### MUST

- UI 展示的是“实际生效的接入点”：
  - `Node quotas` 里如果未勾选任何 endpoint，则用户订阅接口返回 0 条（raw/clash）。
  - 若用户有 4 个 enabled grants，则 `Node quotas` 在对应 node+protocol 上显示为已选择，并且订阅输出 4 条。
- hard cut 行为：
  - `Apply changes` 后，用户实际生效的 enabled grants 与 UI 选择集合完全一致（不允许“额外隐藏 grants”残留）。
- API/UI 层面不出现“组概念”：
  - `Node quotas` 不再请求 `/api/admin/grant-groups/*`，也不再依赖 `managed-<user_id>` 规则。
- 行为确定性：
  - 同输入集合下（endpoints、grants）UI 的默认选中状态稳定一致。

### SHOULD

- hard cut 对存量数据的处理应可诊断（日志/错误信息清晰），避免静默失败。
- 保持与现有 quota 编辑口径一致（继续使用 `user-node-quotas` 系列 API）。

### COULD

- 增加一个只读视图：展示“哪些历史 group 贡献了该用户的 grants”（仅用于排障；默认不在 UI 暴露）。

## 接口契约（Interfaces & Contracts）

- `contracts/http-apis.md`

## 验收标准（Acceptance Criteria）

- Given 用户只有历史 group grants（例如 `group_name="apxdg"`）且 UI `Node quotas` 初始无勾选
  When 打开 `User -> Node quotas`
  Then UI 必须显示实际已生效的勾选状态（与订阅输出条目一致）

- Given 用户存在 4 个 enabled grants
  When 在 `Node quotas` 清空所有选择并点击 `Apply changes`
  Then
  - `GET /api/sub/<token>?format=raw` 返回 0 行
  - `GET /api/sub/<token>?format=clash` 返回 0 proxies
  - 再次打开 `Node quotas` 时仍为全空（无“隐藏 grants”回弹）

- Given 用户选择 N 个 endpoints 并 `Apply changes`
  When 拉取订阅（raw/clash）
  Then 订阅输出条目数 == N，并且 UI 预览的条目数也为 N

## 实现前置条件（Definition of Ready / Preconditions）

- 明确：hard cut 的“清理策略”是 disable 还是 delete：
  - 推荐：delete grants（更符合“无隐藏接入点”，也避免遗留 quota 语义干扰）
- 明确：当同一 node+protocol 有多个 endpoints 时，hard cut 的写入模型（单选 endpoint_id）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Backend：`cargo test`
- Web：`cd web && bun run test`（至少覆盖 Node quotas 与订阅一致性）

## 文档更新（Docs to Update）

- `docs/desgin/subscription.md`（如订阅规则/语义需要补充说明）
- 如有新增 Admin API：更新对应设计文档（或在本计划 contracts 中冻结）

## 计划资产（Plan assets）

- None

## 资产晋升（Asset promotion）

- None

## 实现里程碑（Milestones）

- [ ] M1: 冻结 Admin API 契约（无 group 的 grants 管理）
- [ ] M2: 后端实现 + HTTP tests（hard cut 覆盖历史 grants）
- [ ] M3: Web `Node quotas` 切换到新 API + Web tests
- [ ] M4: 在本地 3 节点回归环境中验证 UI/订阅一致性

## 方案概述（Approach, high-level）

- 将“用户接入点配置”抽象为 **user grants（无 group 概念）**：
  - 读取：按 user 聚合 grants，忽略/兼容历史 `group_name`
  - 写入：hard cut 生成一组目标 grants，替换/清理该 user 的所有历史 grants
- UI 只关心“这个 user 最终能用哪些 endpoints”，而不是“来自哪个 group”。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - hard cut 清理历史 grants 可能影响依赖 group 的其它工作流（需要确认是否仍有人在用）
- 开放问题：
  - 旧的 `grant-groups` 相关 API/页面是否需要后续 deprecate / 移除？
- 假设：
  - “用户可用 endpoints”是权限核心口径，且应该以用户维度可直接编辑与审计。

## 变更记录（Change log）

- 2026-02-02: create plan (frozen)

## 参考（References）

- `web/src/views/UserDetailsPage.tsx`（managed group 假设）
- `src/http/mod.rs`（grant-groups admin API）
- `src/domain/mod.rs:Grant`（`group_name` 为历史字段）
