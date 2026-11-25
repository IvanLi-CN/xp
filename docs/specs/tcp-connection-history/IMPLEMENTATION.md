# Implementation

## Backend

- 待实现：`/proc/net/tcp*` 全量分类采样、runtime 身份解析、控制面端口集合。
- 待实现：本地二进制 raw、旧 JSON 幂等导入与兼容镜像、5m / daily Rollup、保留与 gap、truncated 语义。
- 待实现：`node_tcp_connections.v1` 投递、专用 reducer、容量预检与 suspension。
- 待实现：API 的类别、窗口、分辨率与质量字段。

## Frontend

- 待实现：total 默认线、类别堆叠、outbound_other UID tooltip、30d/90d 窗口。
- 待实现：gap / partial / truncated / unsupported 状态。

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

- [ ] M1: 全量分类采样、本地二进制 raw、旧 JSON 导入与兼容镜像
- [ ] M2: 5m / daily Rollup、保留、gap 与容量护栏
- [ ] M3: `node_tcp_connections.v1`、专用 reducer 与清理
- [ ] M4: API 与 UI 类别/窗口/质量
- [ ] M5: 文档、视觉证据与兼容收口
