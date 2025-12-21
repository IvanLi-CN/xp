# xp · M5 收尾：Quota 强约束与 Raft 一致性（策略 H + Forwarding Writes）

> 对齐：`docs/plan/m5-raft-cluster.md` / `docs/plan/m4-quota-system.md`\
> 参考：`docs/desgin/quota.md` / `docs/desgin/api.md` / `docs/desgin/cluster.md`\
> 本文档位于 `docs/plan/`，作为 Milestone 5 的收尾补充设计。

## 1. 背景与目标

Milestone 4 在单机模式下完成了配额闭环：用量累计（本地持久化）+ 超限封禁 + 周期自动解封。\
Milestone 5 引入 Raft 后，Nodes/Endpoints/Users/Grants 需要通过 leader 串行化写入并复制到所有节点。

当前存在一个语义缺口：配额封禁/解封会修改 `Grant.enabled`，但若直接在本地写入，会绕过 Raft，导致集群各节点看到的期望状态不一致。

本收尾工作的目标：

1. **强一致期望状态**：配额触发的 `Grant.enabled` 变更必须进入 Raft 提交并复制。
2. **强约束优先（策略 H）**：当 Raft 暂不可写时，owner 节点仍必须稳定阻断连接，避免“禁用抖动”。
3. **异常可观测**：当“期望启用”与“数据面实际封禁”短暂不一致时，管理员必须能被明确提示与定位原因。

## 2. 范围与非目标

### 2.1 范围

- Raft 状态机命令：为 `SetGrantEnabled` 增加 `source`（manual/quota），并调整 apply 的副作用规则。
- 写入转发：实现一个通用的 `ForwardingRaftFacade`，对非 leader 的 `client_write` 自动转发到 leader。
- 策略 H：reconcile 在 owner 节点以 `effective_enabled` 为准执行 AddUser/RemoveUser。
- 可观测性：
  - 新增 `GET /api/admin/alerts`（集群级异常提示）。
  - 扩展 `GET /api/admin/grants/{grant_id}/usage`，返回 `effective_enabled` 及相关判定信息。
  - Web（最小）：首页展示 alerts 数量与摘要（不做完整面板）。

### 2.2 非目标

- 不新增新的权限体系（仍使用 admin token）。
- 不改变订阅输出的全局一致性策略（订阅仍以 Raft 期望状态为准）。
- 不做节点移除/缩容等成员变更高级流程。
- 不完成完整 Web 面板（Milestone 6）。

## 3. 术语

- **owner 节点**：某个 Grant 所在 Endpoint 的 `endpoint.node_id` 对应的节点；仅 owner 节点允许对本机 xray 执行该 Endpoint/Grant 的数据面操作（reconcile/quota）。
- **desired_enabled**：Raft 期望状态中的 `Grant.enabled`。
- **quota_banned**：owner 节点本地用量状态中的“配额封禁标记”（不进 Raft）。
- **effective_enabled（策略 H）**：`desired_enabled && !quota_banned`（仅在 owner 节点数据面执行时使用）。

## 4. 状态机命令设计（SetGrantEnabled.source）

### 4.1 新增字段

- `DesiredStateCommand::SetGrantEnabled` 增加字段：
  - `source: manual|quota`

### 4.2 副作用规则（与 M4 兼容）

为满足 “管理员手动变更优先”：

- 当 `source=manual`（管理员显式意图）：
  - apply 后清除本地 `quota_banned`（避免下一周期自动解封误伤人工禁用/启用意图）。
- 当 `source=quota`（系统配额动作）：
  - apply 后不得清除 `quota_banned`。

同时保留现有规则：`UpdateGrantFields` 视为 manual 行为（管理员 PATCH），仍清除 `quota_banned`。

## 5. 写入转发：ForwardingRaftFacade

### 5.1 目标

让所有调用方（HTTP handler、quota worker 等）都只调用 `raft_facade.client_write(cmd)`，无需关心本机角色。

### 5.2 转发策略

- 先在本机尝试 `openraft::Raft::client_write(cmd)`。
- 若返回 `ForwardToLeader`：
  - 优先使用错误里携带的 `leader_node.api_base_url` 定位 leader。
  - 通过 HTTPS（mTLS 优先）调用 leader 上的“写入代理接口”提交同一命令。
- 若 `ForwardToLeader` 未包含 leader 信息：
  - 回退到本机 metrics 的 `leader_api_base_url`（如可用）。

### 5.3 写入代理接口（内部）

为实现转发，新增一个仅管理员可用的内部接口（由 `ForwardingRaftFacade` 调用）：

- `POST /api/admin/_internal/raft/client-write`

请求体：`DesiredStateCommand`（JSON）\
响应体：`raft::types::ClientResponse`（OK/ERR，沿用现有错误形状）。

鉴权：`Authorization: Bearer <admin_token>`。

> 注意：浏览器在跨域 307/308 redirect 场景下可能丢失 `Authorization`，因此这里的转发以“服务端/进程内”完成为准，不要求浏览器跟随跳转。

## 6. Quota 与 Reconcile（策略 H）

### 6.1 Quota 写入顺序（超限封禁）

当超限判定成立时，owner 节点按以下顺序执行：

1. 写入本地用量状态：`quota_banned=true`（并记录 `quota_banned_at`）。
2. 立即对本机 xray 执行 `RemoveUser`（尽快断链）。
3. 通过 `raft_facade.client_write(SetGrantEnabled{enabled=false, source=quota})` 将 `desired_enabled=false` 写入 Raft。
4. `reconcile.request_full()` 触发收敛。

当第 3 步因 Raft 暂不可写失败时：

- 仍以 `quota_banned=true` 保证数据面强约束（策略 H）。
- 记录告警（见 §7），并在后续 tick 重试写入。

### 6.2 周期切换自动解封

当进入新周期且允许自动解封时，owner 节点：

1. `raft_facade.client_write(SetGrantEnabled{enabled=true, source=quota})` 写入 Raft。
2. 清除本地 `quota_banned`。
3. `reconcile.request_full()` 触发收敛。

### 6.3 Reconcile 的执行口径（核心）

在 owner 节点，对每个 grant 的数据面动作以 `effective_enabled` 为准：

- `effective_enabled=true`：确保 AddUser 存在。
- `effective_enabled=false`：确保 RemoveUser 生效（即使 `desired_enabled=true`）。

这样可避免 “RemoveUser 后被 periodic full reconcile 又加回去” 的抖动。

## 7. 异常可观测性（必须提示）

### 7.1 异常定义

当满足以下条件时，视为需要提示的异常：

- `desired_enabled == true`（Raft 期望启用）
- `quota_banned == true`（owner 节点本地配额封禁）
- `effective_enabled == false`（数据面实际禁用）

该异常是策略 H 的预期行为：通常意味着 “Raft 暂不可写/写入滞后” 或 “正在等待下一次重试完成全局禁用”。

### 7.2 集群级 Alerts

新增：

- `GET /api/admin/alerts`

返回集群范围内的异常列表，至少包含：

- `type`: `quota_enforced_but_desired_enabled`
- `grant_id` / `endpoint_id` / `owner_node_id`
- `desired_enabled` / `quota_banned` / `effective_enabled`
- `since`（优先 `quota_banned_at`）
- `message` 与 `action_hint`
- `partial` + `unreachable_nodes`（当部分节点不可达导致结果不完整）

聚合策略（MVP）：请求节点按 `Nodes` 列表逐个拉取各节点的本地异常摘要并聚合；若不可达则标记为 partial。

### 7.3 Grant Usage 扩展

扩展：

- `GET /api/admin/grants/{grant_id}/usage`

除 `cycle_*` / `used_bytes` 外，额外返回：

- `owner_node_id`
- `desired_enabled`
- `quota_banned` / `quota_banned_at`
- `effective_enabled`
- `warning`（当 `desired_enabled != effective_enabled` 时提供明确提示文案）

## 8. 兼容性与迁移

- API 变更为**向后兼容的扩展**（新增字段与新增接口）。
- `usage.json` 继续作为本地状态，不进入 Raft；仅使用既有 `quota_banned` 字段即可满足策略 H。
- 需要更新 `docs/desgin/quota.md` 与 `docs/desgin/api.md` 的口径，明确策略 H 的异常提示与判定规则。

## 9. 风险点与待确认

- 集群互访可达性依赖 `Node.api_base_url`：若部分节点不可达，alerts 需要明确提示 partial。
- admin token 作为内部转发鉴权：需要确保对外暴露面受控（只在可信网络/反代后）。
- leader 频繁切换时的重试与退避策略需避免放大流量（MVP 限定重试次数即可）。

## 10. 测试计划

- 单测：
  - `SetGrantEnabled` 的 apply 副作用：manual 清 ban，quota 不清 ban。
  - `effective_enabled` 逻辑：`quota_banned=true` 时强制 RemoveUser 路径。
- 集成（2–3 节点）：
  - 在非 leader（且为 owner）节点触发超限：能通过转发写入 Raft，并最终全局 `desired_enabled=false`。
  - Raft 暂不可写时：owner 节点仍保持断连，alerts/usage 明确提示异常。
- E2E（可选）：复用现有 xray e2e 验证真实流量增长下的封禁链路。
