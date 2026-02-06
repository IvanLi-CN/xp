# Admin: Delete cluster node（#3hpk4）

## 状态

- Status: 待实现
- Created: 2026-02-06
- Last: 2026-02-06

## 背景 / 问题陈述

- 集群节点在反复 join / 灾难恢复 / 误操作后，可能残留大量“历史节点”（已不可达或不再使用）。
- 这些节点会在 Admin UI 的 Nodes inventory 中持续出现，且可能影响运维判断（例如：正常只应保留 2 台机器，却看到更多 node）。
- 当前缺少一个“正规、可审计”的删除节点能力，用于清理不再属于集群的节点。

## 目标 / 非目标

### Goals

- 提供一个 **公开的管理员 API** 用于删除节点：`DELETE /api/admin/nodes/:node_id`。
- 在 Web UI（Node details）提供可控的删除入口（Danger zone + 二次确认）。
- 删除动作必须同时：
  - 从 Raft membership 中移除该节点（若存在）
  - 从 Raft state machine 的 nodes inventory 中移除该节点（保证 UI/数据一致）
- 提供必要护栏：禁止删除当前 leader、禁止删除当前正在服务的本机节点。

### Non-goals

- 不提供通过 Web/UI 修改 node meta（仍由 xp-ops 配置文件作为唯一来源）。
- 不在本计划中实现“级联删除 endpoint/grant”的一键清理；删除节点前需保证该节点下无 endpoints（避免破坏订阅输出）。

## 范围（Scope）

### In scope

- Server（xp）：
  - 新增 `DELETE /api/admin/nodes/:node_id`（Bearer admin token）。
  - 新增 `DesiredStateCommand::DeleteNode`，并在应用层保证删除不会留下会破坏订阅/配额的悬挂引用：
    - 若该 node 仍存在 endpoints，则拒绝删除（409 conflict）。
    - 删除时清理 `user_node_quotas` 中对该 node 的配额配置（避免 quota worker 读取已删除 node）。
  - 若该 node 属于 Raft membership：
    - 若是 voter：先 `RemoveVoters`（retain=true，使其退化为 learner）
    - 再 `RemoveNodes`（把 learner 从 membership 移除）
  - 为 follower 场景提供 server-side forwarding（内部签名）以避免浏览器跨域/丢 Authorization。
- Web：
  - Node details 页面新增 Danger zone：删除 node（ConfirmDialog 二次确认）。

### Out of scope

- 删除 node 时自动迁移 endpoints / grants 到其他节点。
- 删除 node 时对外暴露 Raft membership 的完整管理面（仅实现所需最小能力）。

## 验收标准（Acceptance Criteria）

- Given 管理员已登录并能查看 Nodes inventory，
  When 在某个非 leader、非本机节点的 Node details 点击 Delete 并确认，
  Then
  - API 返回 `204 No Content`
  - `GET /api/admin/nodes` 不再包含该 node
  - Raft membership 中不再包含该 node（voter/learner 都不存在）

- Given node 仍关联至少 1 个 endpoint，
  When 调用 `DELETE /api/admin/nodes/:node_id`，
  Then 返回 `409 conflict`，且 node 不被删除。

- Given 请求删除当前 leader 或本机节点，
  When 调用 `DELETE /api/admin/nodes/:node_id`，
  Then 返回 `400 invalid_request`，且 node 不被删除。

## 测试与验证（Testing）

- Rust：
  - `cargo test`
  - 新增 HTTP tests 覆盖：
    - delete node 成功（无 endpoints）
    - delete node 冲突（有 endpoints）
    - delete node 拒绝（当前 leader / 本机）
- Web：
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
  - `cd web && bun run test`
- Compose（本地测试环境）：
  - 在 `scripts/dev/subscription-3node-compose/` 环境启动后验证：
    - 不影响现有 `reset-and-verify`
    - 额外手动验证删除一个“无 endpoints 的 node”可成功

## 里程碑（Milestones）

- [ ] M1: Server 端 `DELETE /api/admin/nodes/:node_id` + state command + 保护逻辑
- [ ] M2: Web Node details 增加 delete 入口 + 错误展示
- [ ] M3: 回归验证（单测 + compose env）

