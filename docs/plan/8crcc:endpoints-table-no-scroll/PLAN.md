# Web UI：Endpoints 表格避免横向滚动条（#8crcc）

## 状态

- Status: 已完成
- Created: 2026-02-19
- Last: 2026-02-19

## 背景 / 问题陈述

- Endpoints 列表表格在常见桌面宽度下（侧边栏展开时）出现横向滚动条，影响信息扫描与操作效率。
- 该页面的关键信息主要用于日常运维定位：探测状态、延迟、endpoint/tag、所属节点与端口。

## 目标 / 非目标

### Goals

- 在 Storybook 中模拟 `>=1024px` 的典型桌面场景（主内容宽度 **648px**）时，Endpoints 表格不出现横向滚动条。
- 每条记录的单元格最多两行：每行展示一个字段；字段内容不换行（`nowrap`），超长内容使用省略号截断（`…`）。
- 保留关键交互：
  - Probe bar 可点击进入 endpoint probe 页面；
  - Tag 可点击进入 endpoint details 页面；
  - Endpoint ID 可一键复制（不占用额外列宽）。

### Non-goals

- 不把表格改造成移动端卡片布局；更窄视口允许截断/降级显示（本计划不保证 `375px`）。
- 不改造通用 `DataTable` 行为（仅调整 Endpoints 表格的字段取舍与单元格布局）。

## 方案概述（字段取舍与布局）

- 表格列由 7 列缩减为 4 列：
  1) Probe (24h)
  2) Latency (p50 ms)
  3) Endpoint（两行：Tag / Kind；并提供 copy endpoint_id）
  4) Node（两行：node_id / port）
- Kind 显示短标签：
  - `vless_reality_vision_tcp` -> `VLESS`
  - `ss2022_2022_blake3_aes_128_gcm` -> `SS2022`
- 对可能超长字段（tag/node_id）使用 `truncate` + `title`，避免撑宽导致横向滚动。

## 验收标准（Acceptance Criteria）

- Given Storybook story 中的 648px frame，
  When 渲染 Endpoints 表格，
  Then `.overflow-x-auto` 容器满足 `scrollWidth <= clientWidth`（无横向滚动）。
- Endpoint 单元格：
  - 第 1 行：Tag（可点击进入 details）
  - 第 2 行：Kind（VLESS/SS2022；原始 kind 作为 `title`）
- Node 单元格：
  - 第 1 行：node_id
  - 第 2 行：port
- 每行字段内容不换行（`nowrap`），超长使用省略号截断（`…`）。

## 测试与验证（Testing）

- Web：
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
  - `cd web && bun run test-storybook`

## 里程碑（Milestones）

- [x] M1: 表格列合并设计冻结（本计划）
- [x] M2: 实现 EndpointsTable + EndpointsPage 接入（无横向滚动）
- [x] M3: Storybook 多宽度验证 + test-storybook 通过

## 交付记录（Delivery）

- Web quality gates:
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
  - `cd web && bun run test-storybook`
