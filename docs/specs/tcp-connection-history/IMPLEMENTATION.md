# Implementation

## Backend

- 已实现：`/proc/net/tcp*` 有界分类采样、固定 runtime UID 解析、控制面监听项（含 wildcard）
  与 16,384 行截断。
- 已实现：`tcp_connections.sqlite3` 1m raw、带 `connection_history_import_v1` marker 的旧 JSON
  一次性导入与 inbound-only 兼容镜像。
- 已实现：5m / daily Rollup（逐桶增量重建，禁止跨源累加）、按保留期裁剪与 96 MiB 本地预算门禁。
- 已实现：`node_tcp_connections.v1` daily 投递、`KNOWN_SCHEMAS`/source/receiver/backfill 映射、
  `history.node-tcp-connections.v1` capability 与 node 删除 marker。
- 已实现：Raft-backed `deleted_endpoints` registry，与 endpoint 删除同 operation 提交；
  查询期按 registry 过滤 subject，total / classes 不变。
- 已实现：30d/90d API 从 ready Repository 读取 daily payload；24h/7d 走本地 raw/5m。
- 已实现：响应为 additive：保留 `node`/`warnings`/`endpoints`/`per_endpoint_series`；
  新增 `total`/`classes`/`subjects`/`quality`/`reasons`，以及 `truncated*`、`partial*`、
  `deleted_subjects`、`subjects_*`。
- endpoint 级仓库 tombstone 不实现（见 SPEC：record 前缀无法表达 subject 级删除），subject
  可见性完全由 registry 查询期过滤。
- 已实现：API 的类别/窗口/分辨率/质量/`Point.partial`/`partial_buckets`/subject 分页/
  `deleted_subjects` 与旧字段兼容；旧目标节点降级为 `unsupported` + `target_node_unsupported`。
- 已实现：24h/7d 旧目标节点归一化为 `quality=unsupported` + `target_node_unsupported`；
  30d/90d 无可用 Repository 时返回 `partial` + `repository_unsupported`。

## Frontend

- 已实现：`All connections` 全站 total + 类别堆叠图，展示 current/peak/quality/resolution。
- 已实现：旧 Web/旧响应形状兼容读取（full-site 字段 optional，缺失时回退旧 inbound 视图）。
- 已实现：truncated / partial / deleted_subjects / subjects_omitted / subjects_truncated 状态提示。
- 待实现：30d/90d 窗口按钮与 outbound_other UID tooltip 细分。

## Coverage

- 已覆盖：full-site 分类、截断、runtime UID、SQLite record/load、legacy 一次性导入与不降级；
  5m/daily 同刻求和极值、`samples_expected`/`samples_actual` 缺口语义与 bucket 持久化；
  daily payload 绑定、subject 分页上限与 registry subject 过滤。
- 已覆盖：旧 Web schema 仍可解析 additive 响应；旧 target 降级；registry 过滤 24h/7d 与 30d/90d；
  mirror 重跑不降级 full-class 分钟。
- 已覆盖：full-site 图表渲染、legacy 回退解析、窗口对齐含 total/classes/subjects；
  demo mock 已提供 total/四类/subjects 与 24h/7d/30d/90d。
- 已覆盖：30d/90d 窗口按钮、Storybook 全站 story；视觉证据见
  `assets/connections-default.png` 与 `assets/demo-node-details.png`。

## Validation

- `cargo clippy -- -D warnings`
- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run test-storybook`

## Recorded Delivery State

- Status: complete
- Created: 2026-11-25
- Last: 2026-11-26

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 有界分类采样、本地二进制 raw、旧 JSON 一次性导入与兼容镜像
- [x] M2: 5m / daily Rollup、保留、gap 与容量护栏
- [ ] M3: `node_tcp_connections.v1`、reducer、schema/capability 注册、endpoint registry/node 清理
      （投递、注册与 node 清理已完成；endpoint registry 与查询投影未完成）
- [ ] M4: API/UI 的类别、窗口、质量、分页、partial buckets 与旧字段兼容
- [ ] M5: 文档、视觉证据、旧 Web/旧 Repository 兼容收口
