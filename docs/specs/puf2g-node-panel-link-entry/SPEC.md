# Node panel icon-only entry for node lists (#puf2g)

## Status

- Status: 已完成
- Created: 2026-03-01
- Last: 2026-03-01

## Background

- 节点列表的“打开节点面板”入口在视觉上不够明确：`Nodes` 页当前依赖名称/Node ID 文本链接，`Dashboard` 页节点表没有直达入口。
- 管理员在排障时需要快速进入节点详情页（`/nodes/$nodeId`），应提供统一且显式的 icon 入口。

## Goals / Non-goals

### Goals

- 在 `Nodes` 页和 `Dashboard` 节点表提供统一的 icon-only “Open node panel”超链接入口。
- `Nodes` 页节点名称与 Node ID 改为纯文本，仅保留 icon 作为唯一跳转入口。
- icon 入口具备可访问性标签（`title` + `aria-label`）并保持键盘可达。

### Non-goals

- 不新增/修改后端 API。
- 不新增/修改前端路由。
- 不扩展到其他页面（仅 `Nodes` + `Dashboard`）。

## Scope

### In scope

- `web/src/views/NodesPage.tsx`：节点行重构为 icon-only 跳转入口。
- `web/src/views/HomePage.tsx`：Dashboard 节点表新增 icon-only 跳转入口。
- `web/src/views/NodesPage.test.tsx`：新增行级入口行为测试。
- `web/src/views/HomePage.test.tsx`：新增 Dashboard 行级入口行为测试。
- `web/src/views/NodesPage.stories.tsx`：补充 Storybook play 断言，避免入口回归。

### Out of scope

- 节点详情页内容与交互。
- 其他包含 Node ID 展示的页面。

## UI Contract

- 入口 icon：
  - Icon: `tabler:external-link`
  - Size: `16`
  - Class: `btn btn-ghost btn-xs btn-square shrink-0`
- 文案：
  - `title` / `aria-label`: `Open node panel: <node_name_or_node_id>`
- 跳转目标：
  - `to="/nodes/$nodeId"`
  - `params={{ nodeId }}`
- 布局：
  - 名称 + icon 同行：`flex items-center gap-2 min-w-0`
  - 名称文本保留截断，icon 不允许被挤压。

## Acceptance criteria

- Given 管理员进入 `Nodes` 页面，When 查看任意节点行，Then 仅存在一个 icon 入口可跳转到该节点详情页，名称与 Node ID 不是链接。
- Given 管理员进入 `Dashboard` 页面，When 查看 Nodes 表格，Then 每行节点名称右侧都存在 icon 入口并跳转到对应节点详情页。
- Given 键盘用户聚焦 icon 入口，When 按 Enter，Then 可触发导航到对应节点详情。
- Given 视觉回归检查，When 对比 `Nodes` 与 `Dashboard`，Then 两处入口样式与可访问性文案一致。

## Tests

- Unit tests (Vitest + RTL):
  - `web/src/views/NodesPage.test.tsx`
  - `web/src/views/HomePage.test.tsx`
- Storybook guard:
  - `web/src/views/NodesPage.stories.tsx` play 断言 icon link 数量。
- Validation commands:
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
  - `cd web && bun run test`

## Milestones

- [x] M1: 新建规格并冻结 UI 行为口径
- [x] M2: Nodes 页面改为 icon-only 节点面板入口
- [x] M3: Dashboard 节点表补齐 icon-only 节点面板入口
- [x] M4: 单测 + Storybook 回归断言补齐并通过验证

## Risks / Notes

- 文案与按钮语义统一为英文（`Open node panel`），与当前 UI 文案风格保持一致。
- 本变更只调整入口交互，不改变任何业务数据或权限语义。

## Change log

- 2026-03-01: added icon-only node panel links for Nodes and Dashboard lists, with tests and storybook guard.
- 2026-03-01: opened PR #87.
