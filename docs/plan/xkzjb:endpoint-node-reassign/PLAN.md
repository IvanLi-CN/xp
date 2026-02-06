# Admin: Reassign endpoint node（#xkzjb）

## 状态

- Status: 待实现
- Created: 2026-02-06
- Last: 2026-02-06

## 背景 / 问题陈述

- Endpoint 与 Grant 都是按 `node_id` 关联的（订阅输出依赖 `Node.access_host`，下发到 xray 的 inbound 归属也依赖 endpoint 的 `node_id`）。
- 当某台机器被重置（数据清空）后重新加入集群，会生成新的 `node_id`。此时：
  - 历史 endpoints/grants 仍指向旧 `node_id`，导致订阅输出 host/端口不符合当前真实节点；
  - “删除历史节点（node inventory clean-up）”会因为该 node 仍被 endpoints 引用而被拒绝（409）。
- 目前缺少一个“正规、可审计”的方式，把既有 endpoint **从旧 node 迁移到新 node**，以便完成灾难恢复/重建后的收敛与清理。

## 目标 / 非目标

### Goals

- 提供一个 **公开的管理员 API** 支持将 endpoint 迁移到另一个 node（不改变 endpoint_id / tag / meta）：
  - `PATCH /api/admin/endpoints/:endpoint_id` 支持可选字段 `node_id`
- 迁移必须具备护栏：
  - 目标 node 必须存在于 nodes inventory（Raft state machine）
  - 目标 node 上不允许出现端口冲突（同一 node 上不能有两个 endpoints 绑定同一 port）
- 迁移后应触发 reconcile，保证 xray inbound 能在合理时间内重建/收敛。

### Non-goals

- 不在本计划中实现“批量迁移 endpoints”的一键工具（先满足单 endpoint 迁移；批量可后续通过脚本/重复操作完成）。
- 不在本计划中修改 node meta 的编辑策略（node meta 仍由 xp-ops 配置文件作为唯一来源）。
- 不在本计划中实现“级联删除 endpoints/grants”的强制清理能力。

## 范围（Scope）

### In scope

- Server（xp）：
  - `PATCH /api/admin/endpoints/:endpoint_id` 新增可选字段：
    - `node_id`（string, optional）
  - 校验：
    - `node_id` 非空且目标 node 存在
    - 迁移后在目标 node 上不产生端口冲突（同 port 的其他 endpoint）
  - 迁移行为：
    - 仅更新 endpoint 的 `node_id`，其余字段（`endpoint_id/tag/kind/meta/port`）保持不变
    - 通过 Raft `UpsertEndpoint` 落盘并复制
    - 触发 reconcile full

### Out of scope

- Web UI 增加 “Move endpoint” 的交互入口（可按实际运维需求再补；当前可通过 API/脚本完成迁移）。

## 验收标准（Acceptance Criteria）

- Given endpoint A 存在且归属 node_old，
  When 调用 `PATCH /api/admin/endpoints/:endpoint_id` 并提供 `node_id=node_new`，
  Then
  - API 返回 `200 OK`
  - 响应中的 `node_id` 变为 `node_new`
  - endpoint 的 `endpoint_id/tag/meta` 不发生变化
  - `GET /api/admin/endpoints/:endpoint_id` 返回更新后的 `node_id`

- Given `node_new` 不存在，
  When 迁移 endpoint，
  Then 返回 `400 invalid_request`（或 `404 not_found`，按实现选择其一），且 endpoint 不被修改。

- Given `node_new` 上已存在另一个 endpoint 占用同一 `port`，
  When 迁移 endpoint，
  Then 返回 `409 conflict`，且 endpoint 不被修改。

## 测试与验证（Testing）

- Rust：
  - `cargo test`
  - 新增 HTTP tests 覆盖：
    - endpoint 迁移成功（node_id 更新，meta 保持）
    - endpoint 迁移拒绝（目标 node 不存在）
    - endpoint 迁移冲突（目标 node 端口冲突）
- Compose（本地测试环境）：
  - 在 `scripts/dev/subscription-3node-compose/` 环境中验证：
    - 迁移 endpoint 后，订阅输出指向新 node 的 access_host
    - 删除旧 node（确保旧 node 无 endpoints）可成功

## 里程碑（Milestones）

- [ ] M1: Server 端支持 patch endpoint node_id + 校验
- [ ] M2: 单测/HTTP tests 覆盖 + 回归
- [ ] M3: Compose 环境验证（手动）

## 风险与开放问题

- 风险：迁移 endpoint 可能会改变用户侧“同一 grant 对应的出口节点”，需要运维确认迁移窗口（但不会改变 endpoint_id/tag/meta，客户端刷新订阅即可获取新 host）。
- 开放问题：是否需要限制目标 node 必须在 Raft membership 中（当前先只要求 nodes inventory 存在，以保持灵活性）。
