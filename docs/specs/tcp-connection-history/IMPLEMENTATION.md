# Implementation

## Backend

- 待实现：`/proc/net/tcp*` 有界分类采样、runtime 身份解析、控制面监听项（含 wildcard）。
- 待实现：本地二进制 raw、旧 JSON 幂等导入（`classes_observed`）与兼容镜像、5m / daily Rollup。
- 待实现：`node_tcp_connections.v1` 投递、专用 reducer、schema 注册、source/receiver 映射、
  查询投影与 node/endpoint 删除 marker。
- 待实现：API 的类别、窗口、分辨率、质量、分页与 `deleted_subjects` 字段。

## Frontend

- 待实现：total 默认线、类别堆叠、outbound_other UID tooltip、30d/90d 窗口。
- 待实现：gap / partial / truncated / unsupported / capture_suspended 状态。

## Coverage

- 待实现。

## Validation

- `cargo fmt --all`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run test-storybook`

## Recorded Delivery State

- Status: 未开始
- Created: 2026-11-25
- Last: 2026-11-25

## 实现里程碑（Milestones / Delivery checklist）

- [ ] M1: 有界分类采样、本地二进制 raw、旧 JSON 导入与兼容镜像
- [ ] M2: 5m / daily Rollup、保留、gap 与容量护栏
- [ ] M3: `node_tcp_connections.v1`、reducer、schema 注册、endpoint/node 清理
- [ ] M4: API 与 UI 的类别/窗口/质量/分页
- [ ] M5: 文档、视觉证据与兼容收口
