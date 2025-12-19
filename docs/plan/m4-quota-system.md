# Milestone 4 · 配额系统（单机强约束）— 需求与概要设计

> 对齐计划：`docs/plan/README.md` 的 **Milestone 4**。\
> 参考：`docs/desgin/quota.md` / `docs/desgin/api.md` / `docs/desgin/architecture.md`

## 1. 背景与目标

Milestone 1–3 已完成：

- 期望状态（Nodes/Endpoints/Users/Grants）可持久化；
- 本机 xray 运行态收敛（inbounds/clients）；
- 订阅输出可交付。

但目前仍缺少 MVP 的强约束能力：**配额统计与自动封禁/解封**。Milestone 4 的目标是在**单机模式**下形成可交付的闭环：

- 按 `quota_limit_bytes` 对每个 Grant 做双向合计用量统计；
- 超限后尽快断开/拒绝连接（移除 client），并把 `Grant.enabled=false` 持久化；
- 到达下一周期后按策略自动恢复（或保持封禁），确保行为可预测。

## 2. 范围与非目标

### 2.1 范围（M4 交付）

- **用量采集（后台轮询）**：周期性从 xray `StatsService` 拉取每个 Grant 的 uplink/downlink 累计值，并做增量累计（用量落本地文件，不进 Raft）。
- **周期窗口计算**：支持 `ByUser(UTC+8)` 与 `ByNode(本地时区)`，并遵循“缺日取月末”。
- **超限动作**（当 `quota_limit_bytes > 0`）：
  1. 立即对目标 inbound 执行 `RemoveUserOperation`；
  2. 将 `Grant.enabled=false` 写入期望状态并持久化；
  3. 记录“此禁用来自配额”（用于后续自动解封判定）。
- **周期切换与自动解封（可配置）**：到达下一周期时，重置 `used_bytes` 并按策略将“配额封禁”的 Grant 自动设回 `enabled=true`，由 reconcile 重新 AddUser。

### 2.2 非目标（明确不做）

- Raft 集群一致性写入（Milestone 5 才引入）。本里程碑的“写入期望状态”仅指单机持久化到 `state.json`。
- 跨节点汇总用量与代理查询（集群版本再做）。
- 复杂的 token/权限体系、封禁黑名单等扩展策略（MVP 不引入）。

## 3. 关键用例 / 用户流程

1. 正常用量累计：
   - xp 周期性拉取 stats → 更新 `usage.json` → `GET /api/admin/grants/{id}/usage` 展示 `used_bytes`。
2. 超限封禁：
   - `used_bytes + 10MiB >= quota_limit_bytes` → RemoveUser（尽快断链）→ `Grant.enabled=false`（持久化）→ reconcile 收敛确保不可连。
3. 周期切换自动解封：
   - `now >= cycle_end_at` → 进入新周期窗口 → `used_bytes=0` → 若该 Grant 为“配额封禁”且启用自动解封 → `Grant.enabled=true` → reconcile AddUser。
4. 管理员手动启用/禁用与配额的交互：
   - 管理员显式修改 `Grant.enabled` 时，系统应视为人工意图，避免下一周期“自动解封”误伤（详见 §4.3）。

## 4. 数据 / 领域模型变更

### 4.1 现状

- 期望状态（`state.json`）：`Grant.enabled/quota_limit_bytes/cycle_policy...` 等字段已存在。
- 本地用量（`usage.json`）：已包含 `cycle_start_at/cycle_end_at/used_bytes/last_*` 等字段，并实现“计数回退不倒扣”的增量逻辑。

### 4.2 新增：本地“配额封禁来源”标记

为区分“管理员手动禁用”与“系统因配额禁用”，需要在本地用量状态中引入一个可恢复的标记（不进 Raft）：

- `quota_banned: bool`（默认 false）
- （可选）`quota_banned_at: string`（RFC3339，用于排查）

它只用于驱动“自动解封”的判定；真正是否可连仍以 `Grant.enabled` 为准。

### 4.3 管理员手动变更的约束

- 当管理员通过 API 显式设置 `Grant.enabled` 时，系统应清除 `quota_banned`（让人工意图优先生效）。
- 当系统因超限将 `Grant.enabled=false` 时，应同步置 `quota_banned=true`。

## 5. 接口与模块边界

### 5.1 对外接口

沿用现有接口：

- `GET /api/admin/grants/{grant_id}/usage`：返回 `cycle_start_at/cycle_end_at/used_bytes`（本里程碑后应能“无人工触发也持续更新”）。

不新增对外 API（MVP 简化）。

### 5.2 内部模块与依赖方向（建议）

新增模块（建议名：`quota`）承载后台轮询与封禁策略，保持边界清晰：

- `quota`：
  - 周期性：按配置间隔 tick
  - 读取：从 `state` 获取 Grants/Endpoints/Users（只读快照）
  - 拉取：通过 `xray` 拉取 stats
  - 写入：更新 `usage.json`；必要时更新 `Grant.enabled` 并触发 `reconcile`
- `reconcile`：继续只负责“期望状态 → xray 运行态”
- `xray`：保持为纯适配层
- `state`：提供必要的读写方法（含“配额封禁标记”的维护）

依赖方向建议：

`http -> state (+ quota handle?)`\
`quota -> (state + xray + reconcile)`\
`reconcile -> (state + xray)`

## 6. 配置与默认值

新增建议（CLI/ENV 二选一即可，保持与现有 config 风格一致）：

- `--quota-poll-interval-secs`（ENV：`XP_QUOTA_POLL_INTERVAL_SECS`）
  - 默认：10
  - 允许范围：5–30（超范围报错或 clamp，需实现时确认）
- `--quota-auto-unban`（ENV：`XP_QUOTA_AUTO_UNBAN`）
  - 默认：true
- 误差容忍：固定 `10MiB`（`docs/desgin/quota.md` 已定）

## 7. 行为细节与错误处理

- Stats 缺失：按 0 处理（Grant 尚未创建连接/或 xray 未产生统计时）。
- xray 不可达：轮询任务应记录日志并退避重试；不应阻塞 HTTP 服务。
- 幂等性：RemoveUser/禁用操作应允许重复执行；“已不存在/已禁用”视为成功。

## 8. 测试计划（M4）

- 单测：
  - 增量累计：正向增长、计数回退（xray 重启）不倒扣
  - 超限封禁：达到阈值后触发 RemoveUser + `Grant.enabled=false` + `quota_banned=true`
  - 周期切换：窗口变化时 `used_bytes` 重置，且只对 `quota_banned=true` 的 Grant 自动解封
  - 管理员显式禁用：清除 `quota_banned`，下一周期不自动解封
- E2E（依赖本机 xray）：复用现有 e2e 脚本/测试，在真实统计增长下验证封禁/解封链路（至少覆盖 SS2022 的超限封禁与周期切换自动解封）。

## 9. 风险点与待确认问题

1. 轮询粒度与性能：Grant 数量增大时是否需要 `QueryStats` 批量拉取（本里程碑可先逐个拉取）。
2. 自动解封默认值：建议默认开启，但是否需要“全局开关 + 每 Grant 覆盖”？
3. 手动禁用的语义：管理员在超限后再次禁用/启用时，对 `quota_banned` 的处理以“人工优先”为推荐方案。
