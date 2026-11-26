# Implementation

## Backend

- 已实现：`/proc/net/tcp*` 有界分类采样、固定 runtime UID 解析、控制面监听项（含 wildcard）
  与 16,384 行截断。
- 已实现：`tcp_connections.sqlite3` 1m raw、带 `connection_history_import_v1` marker 的旧 JSON
  一次性导入与 inbound-only 兼容镜像。
- 待实现：5m / daily Rollup、保留层压缩与 96 MiB 本地预算执行。
- 待实现：`node_tcp_connections.v1` 投递、专用 reducer、schema 注册、source/receiver 映射、
  查询投影、`history.node-tcp-connections.v1` repository capability 与 endpoint/node 删除 marker。
- 待实现：Raft-backed `deleted_endpoints` registry，与 endpoint 删除同 operation 提交；查询按
  registry 过滤 subject，marker 发布失败不阻断查询过滤。
- 待实现：API 的类别、窗口、分辨率、质量、`Point.partial` / `partial_buckets`、64 KiB subject
  分页、`deleted_subjects`，以及旧 `warnings` / `endpoints` / `per_endpoint_series` 兼容字段。
- 待实现：24h/7d 旧目标节点归一化为 `quality=unsupported` + `target_node_unsupported`；
  30d/90d 旧 Repository 返回 `partial` + `repository_unsupported`。

## Frontend

- 待实现：total 默认线、类别堆叠、outbound_other UID tooltip、30d/90d 窗口。
- 待实现：旧 Web/旧响应形状兼容读取，新字段按 capability 降级；endpoint 元数据关联。
- 待实现：gap / partial / truncated / unsupported / capture_suspended 状态。

## Coverage

- 已覆盖：full-site 分类、截断、runtime UID、SQLite record/load、legacy 一次性导入与不降级。
- 待实现：旧 Web schema 解析 24h/7d 新响应；旧 target/repository 降级；endpoint registry
  过滤 24h/7d 与 30d/90d；mirror 重跑不降级 full-class 分钟。

## Validation

- `cargo clippy -- -D warnings`
- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run test-storybook`

## Recorded Delivery State

- Status: in progress (M1 complete)
- Created: 2026-11-25
- Last: 2026-11-26

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 有界分类采样、本地二进制 raw、旧 JSON 一次性导入与兼容镜像
- [ ] M2: 5m / daily Rollup、保留、gap 与容量护栏
- [ ] M3: `node_tcp_connections.v1`、reducer、schema/capability 注册、endpoint registry/node 清理
- [ ] M4: API/UI 的类别、窗口、质量、分页、partial buckets 与旧字段兼容
- [ ] M5: 文档、视觉证据、旧 Web/旧 Repository 兼容收口
