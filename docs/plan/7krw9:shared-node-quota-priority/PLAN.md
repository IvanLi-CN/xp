# 三档优先级共享节点额度（P1/P2 固定 + P1 抢占 P2 节奏溢出 + P3 捡漏）（#7krw9）

## 状态

- Status: 待实现
- Created: 2026-02-19
- Last: 2026-02-19

## 背景 / 问题陈述

- 现有 quota 体系以“每个用户/接入点的固定 quota_limit”为核心，无法表达“多用户按优先级共享同一节点的周期额度”。
- 需求希望支持三档优先级（P1/P2/P3）：
  - P1：限制更少，尽量在周期内持续可用（更强的积累能力），并可拿到 P2 用不掉的节奏溢出。
  - P2：均衡，有固定保留额度，但只允许“按节奏用不掉”的部分被 P1 抢占。
  - P3：捡漏，无固定保留额度，只能使用 P1/P2 的富余溢出。
- 节点的额度与重置时间（周期）可能不同；同一用户可能拥有多个节点的接入点权限。
- 约束：**禁止用户查询自己的 weight**；weight 仅管理员可见，并且 **weight 需要支持按节点设置**。

## 目标 / 非目标

### Goals

- 每个 Node 支持配置其“周期总额度”：`quota_limit_bytes`（0 表示 unlimited）。
- 在单节点本地 enforce 的前提下，实现三档优先级共享：
  - P1/P2 共同参与固定 base quota 的分配（切满可分配额度，扣除 buffer）。
  - P2 的“节奏 cap 以上溢出”流入 P1（不影响 P2 按节奏使用）。
  - P1 的溢出流入 P3；P3 无固定额度，只在有溢出时才可用。
- 支持 `weight(user,node)`：按节点设置、默认值可回退（缺失视为 100），仅 Admin API/UI 可读写。
- 对用户侧（订阅与非 admin API）**零暴露**：不得返回 tier/weight。

### Non-goals

- 不做跨节点全局一致的“统一共享池”（每个 Node 独立周期与额度）。
- 不在本计划内引入用户自助查询/自助调整（仅 admin 管理）。

## 范围（Scope）

### In scope

- 数据模型：
  - Node: `quota_limit_bytes: u64`
  - User: `priority_tier: p1|p2|p3`
  - `user_node_weights`: (user_id, node_id) -> weight
- Admin API（仅 `/api/admin/*`）：
  - `PATCH /api/admin/nodes/:node_id`：更新 `quota_limit_bytes` 与 `quota_reset`
  - `PATCH /api/admin/users/:user_id`：更新 `priority_tier`（保留 display_name/quota_reset 能力）
  - `PUT /api/admin/users/:user_id/node-weights/:node_id`：设置 weight
  - `GET /api/admin/users/:user_id/node-weights`：列出该用户所有 node weights
- Quota worker / reconcile：
  - 新增“按 Node 周期总额度 + P1/P2 权重分配 + 节奏/溢出链”的本地策略与测试。
  - **禁止**因该策略写 Raft `SetGrantEnabled(false)`（避免全局永久禁用）。
  - 本地封禁通过 `usage.json` 的标记驱动（并从 xray inbound 移除用户）。
- Web Admin：
  - 移除旧的 quota 编辑入口（避免绕过策略写静态 quota）。
  - 新增 Quota Policy 页面（管理员可编辑 Node quota_limit_bytes/reset、User tier、User×Node weight）。

### Out of scope

- 对非 admin API 的新增字段（用户侧保持透明）。
- 复杂的“按组/按业务线”分池（本计划不引入）。

## 策略冻结（Allocation & Pacing）

### Base quota（P1/P2 切满）

- Node 每周期总额度：`node.quota_limit_bytes`（0=unlimited -> skip enforcement）。
- `buffer = max(256MiB, floor(quota_limit_bytes * 0.5%))`
- `distributable = quota_limit_bytes - buffer`
- 参与者集合 `U12(node)`：在该 node 有接入权限且 tier ∈ {P1,P2} 的用户。
- `base_quota(u,node) = floor(distributable * w(u,node) / sum_w)`，余数按稳定顺序补齐，确保总和 == distributable。

### Pacing（token bank）

- 按 node 的 quota_reset 计算 cycle window，并得到周期天数 `D`。
- daily credit：`base_quota` 均分到 D 天（前 rem 天 +1）。
- carry_days：
  - P1 = 7
  - P2 = 2
  - P3 = 0
- cap(today) = 最近 carry_days 天的 credit 之和（sliding window）。

### Overflow chain（P1 抢占 P2 节奏溢出 + P3 捡漏）

- P2 若因 cap 限制产生 overflow（bank > cap）：进入 `p1_overflow_pool(node)`，再按 P1 权重分配给 P1（bonus bank）。
- P1 若产生 overflow：进入 `p3_overflow_pool(node)`，按 P3 权重分配给 P3 当日 bank（carry=0，次日过期）。
- 语义保证：P1 仅获得 P2 **按节奏已用不掉**的溢出，不影响 P2 按节奏使用。

## 验收标准（Acceptance Criteria）

- 切满：对任一 node，P1+P2 的 `base_quota` 总和 == `distributable`。
- 抢占范围正确：只有 P2 的 overflow 会进入 P1 bonus（不会吞掉 P2 当日 cap 内可用部分）。
- P2 不受影响：P2 在 cap 内使用不会因 P1 活跃而被提前 ban。
- P3 捡漏：仅当存在 P1/P2 overflow 时 P3 才可用；否则 P3 当日 bank 为 0。
- 隐私：订阅与非 admin API 不返回 tier/weight（尤其 weight 必须 0 暴露）。
- 旧入口消失：旧 quota 编辑 UI 与旧写接口不再可用/不可绕过策略写静态 quota。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Backend：`cargo test`（覆盖分配、余数补齐、overflow chain、不同 node weight）
- Web：`cd web && bun run lint && bun run typecheck && bun run test`

## 实现里程碑（Milestones）

- [ ] M1: 数据模型与 Admin API 契约落地（Node quota_limit_bytes / User tier / User×Node weight）
- [ ] M2: Quota engine 纯函数 + 单测（base/pacing/overflow）
- [ ] M3: Quota worker + reconcile 集成（本地 ban/remove_user，不写 Raft disable）
- [ ] M4: Web Admin Quota Policy 页面 + 移除旧 quota 编辑入口

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：策略从“静态 user quota”迁移到“动态分配”会影响既有 UI/接口，需要明确 deprecate 行为。
- 假设：每个 node 独立 enforce，且用户跨 node 使用时按各 node 周期分别计算。

## 变更记录（Change log）

- 2026-02-19: create plan (frozen)

