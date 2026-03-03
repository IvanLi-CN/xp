# Dashboard/Nodes 共享节点列表与图标链接重构（#gj4xg）

## 状态

- Status: 已完成
- Created: 2026-03-03
- Last: 2026-03-03

## 背景 / 问题陈述

- `Dashboard` 与 `Nodes` 页当前使用不同节点数据源与不同列表结构，展示口径存在偏差。
- 节点详情入口虽然是 icon-only，但实现仍采用按钮语义样式，视觉反馈与“纯链接”要求不一致。
- 窄视口下列表信息密度高，`api_base_url/access_host` 可读性不足。

## 目标 / 非目标

### Goals

- 合并 `Dashboard` 与 `Nodes` 的节点列表实现，复用同一个渲染组件与同一数据源（`/api/admin/nodes/runtime`）。
- 节点详情入口改为纯图标链接，去掉按钮样式，仅悬浮/聚焦时图标变色。
- 提供统一“混合布局”：同时展示节点元数据与运行态（components + 7d slots）。
- 在不同视口下保持可读：桌面表格、移动端卡片。

### Non-goals

- 不修改后端 API、鉴权与路由。
- 不调整 `Nodes` 页 Join token 相关交互。
- 不扩展到 `Dashboard/Nodes` 之外页面。

## 范围（Scope）

### In scope

- 新增共享组件：`web/src/components/NodeInventoryList.tsx`。
- `web/src/views/HomePage.tsx` 改为使用 runtime 节点查询，并接入共享列表组件。
- `web/src/views/NodesPage.tsx` 复用共享列表组件，移除重复列表渲染实现。
- 测试与 mock 同步：`HomePage.test.tsx`、`NodesPage.test.tsx`、新增共享组件测试，补齐 runtime mock 场景。

### Out of scope

- NodeDetails 页面交互。
- 运行态统计逻辑本身（仅展示层重构）。

## 需求（Requirements）

### MUST

- 两页节点列表来自同一接口与同一排序规则。
- 节点详情入口必须是 `<a>` 图标链接，非按钮语义样式。
- 支持窄屏卡片布局，且 `api_base_url/access_host` 不被遮挡或不可读。

### SHOULD

- 两页空态/错误态文案尽量保持一致口径。
- 保留原有可访问性属性（`title` + `aria-label`）。

### COULD

- 后续将更多节点表格页签也迁移到同一组件。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 管理员访问 `Dashboard` 与 `Nodes` 时，节点区块渲染同结构的共享列表。
- 每行显示：`node_name`（含图标链接）、`node_id`、`api_base_url`、`access_host`、异常组件、7d 运行槽位条。
- 点击图标链接跳转 `/nodes/$nodeId`。
- 刷新动作通过共享列表的 refresh 入口触发 query refetch。

### Edge cases / errors

- 无 admin token：展示 token 缺失提示，不发起节点查询。
- runtime 接口报错：展示错误态并提供重试。
- 节点为空：展示空态。
- runtime `partial=true`：展示 unreachable_nodes 警告。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）               | 类型（Kind）       | 范围（Scope） | 变更（Change）          | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                      |
| -------------------------- | ------------------ | ------------- | ----------------------- | ------------------------ | --------------- | ------------------- | ---------------------------------- |
| `/api/admin/nodes/runtime` | HTTP API           | internal      | Modify (frontend usage) | None                     | Web UI          | HomePage/NodesPage  | 后端接口不变，仅两页统一改为该接口 |
| `NodeInventoryListProps`   | UI component props | internal      | New                     | None                     | Web UI          | HomePage/NodesPage  | 统一列表渲染契约                   |

### 契约文档（按 Kind 拆分）

- None

## 验收标准（Acceptance Criteria）

- Given 管理员进入 `Dashboard` 和 `Nodes`，When 节点列表加载成功，Then 两页展示相同数量与顺序的节点。
- Given 任意节点行，When 查看“打开节点面板”入口，Then 入口是 icon-only 链接（非按钮样式），`title/aria-label` 为 `Open node panel: <node_name_or_node_id>`。
- Given 视口宽度小于 `md`，When 查看节点列表，Then 以卡片布局展示且 URL/Host 字段可完整阅读（允许换行）。
- Given runtime 返回 `partial=true`，When 列表渲染，Then 显示 unreachable_nodes 提示。

## 实现前置条件（Definition of Ready / Preconditions）

- 目标/范围/验收口径已冻结。
- 无后端接口变更依赖。
- 相关页面与测试文件已定位，可直接实施。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `HomePage.test.tsx`、`NodesPage.test.tsx`、`NodeInventoryList.test.tsx`
- Integration tests: 复用页面级 RTL 场景
- E2E tests (if applicable): `navigation.spec.ts`（保证节点页可加载）

### UI / Storybook (if applicable)

- Stories to add/update: `NodesPage.stories.tsx`
- Visual regression baseline changes (if any): None

### Quality checks

- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 增加 spec index 行并更新状态。

## 计划资产（Plan assets）

- Directory: `docs/specs/gj4xg-dashboard-nodes-shared-list/assets/`
- In-plan references: None

## 资产晋升（Asset promotion）

- None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建共享 `NodeInventoryList`，覆盖桌面表格 + 移动卡片 + 图标纯链接行为
- [x] M2: Home/Nodes 页面统一切换到 runtime 查询并复用共享列表
- [x] M3: 完成测试与 mock 同步，验证 lint/typecheck/test 全通过
- [x] M4: 快车道完成 PR + checks + review-loop 收敛并回写 spec 状态

## 方案概述（Approach, high-level）

- 抽离现有 `NodesPage` 运行态展示逻辑到共享组件，统一渲染与响应式策略。
- `HomePage` 从 `fetchAdminNodes` 迁移到 `fetchAdminNodesRuntime`，与 `NodesPage` 统一 query key。
- 通过测试断言防止入口语义回退为按钮，确保两页节点顺序与展示字段一致。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：`HomePage` 切换 runtime 后，若后端局部不可达会出现 partial 提示，需确认文案可接受。
- 需要决策的问题：None。
- 假设（需主人确认）：沿用当前主题 token（`hover:text-primary`）作为图标交互色。

## 变更记录（Change log）

- 2026-03-03: 初始规格创建，冻结“共享列表 + 图标纯链接 + 响应式卡片”口径。
- 2026-03-03: 完成共享列表实现与页面接入，新增/更新单测并补齐 e2e mock 路由。
- 2026-03-03: 修复 `ResizeObserver` 缺失时的降级渲染风险，补充对应单测。
- 2026-03-03: PR #93 checks 全部通过（`pr-label-gate` / `ci` / `xray-e2e`），完成快车道收敛。

## 参考（References）

- `docs/specs/puf2g-node-panel-link-entry/SPEC.md`
