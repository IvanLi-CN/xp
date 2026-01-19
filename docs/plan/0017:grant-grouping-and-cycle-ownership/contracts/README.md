# Contracts（#0017）

本目录冻结 #0017 计划涉及的跨边界契约（HTTP API / 持久化文件格式等）。

## 范围

- Admin HTTP APIs：
  - `grant-groups`：仅 group-level 交互；不提供 grants API
  - `users` / `nodes`：新增 `quota_reset`（按月|无限 + 时区）
  - `users/{user_id}/node-quotas`：节点行选择参考 user/node（`quota_reset_source`）
- Persisted state：`state.json` 的 `schema_version` 迁移与字段变更。

## 关键决策点（已冻结）

- 分组（group）口径已定案：A1（name-as-key，`group_name` 唯一且可改名；改名=服务端批量重写）。
- 流量重置/周期/时区口径已定案：grant 不承载；User/Node 都可配置；节点行可选参考 user 或 node（默认 user）。

## Files

- [http-apis.md](./http-apis.md)
- [file-formats.md](./file-formats.md)
