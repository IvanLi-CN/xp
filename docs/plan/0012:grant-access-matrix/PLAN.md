# Grant 接入点配置：协议 × 节点二维表格（#0012）

## 状态

- Status: 已完成
- Created: 2026-01-14
- Last: 2026-01-16

## 1) 问题陈述

当前 Web 管理端创建 Grant 的方式是“选择 `endpoint_id`”的单维度下拉框（见 `web/src/views/GrantNewPage.tsx`）。当节点数与协议（Endpoint.kind）增多时，运营/管理员很难快速理解“某个用户当前到底允许通过哪些节点、哪些协议接入”，也很难进行批量调整。

本计划目标是把“允许的接入点”配置改为二维表格：考虑到“节点数量通常多于协议”，采用 **行=节点（Node）**、**列=协议（Protocol / Endpoint.kind）**；每个单元格是一个可选状态（允许/不允许），并支持对行头、列头、左上角全局头进行批量选择操作，且批量操作规则固定为：**优先取消选择，其次全选，不提供反选**。

## 2) 目标 / 非目标

### Goals

- 提供“节点 × 协议”的接入点二维配置表，能直观看到当前允许接入的组合，并能快速批量调整。
- 行头、列头与左上角头支持批量操作，且批量操作规则一致：
  - 若该组（行/列/全部）**存在任意已选**：点击后执行“全部取消选择”
  - 否则：点击后执行“全部选中”
- 方案能够落地到现有数据模型（`Grant.endpoint_id`）之上：在实现阶段明确“单元格代表什么对象、如何映射到具体 endpoint/grant”，并保证可测试。
- 先交付低保真草图以对齐交互与信息架构；主人确认后再进入高保真 UI 设计与实现阶段。

### Non-goals

- 不在本计划阶段修改任何业务源码、API 或运行配置。
- 不引入新的权限体系（仍沿用 admin token）。
- 不在本计划中重做 Grant 的配额、周期策略等编辑体验（除非主人明确要求把这些也并入矩阵交互）。

## 3) 用户与场景

- **主要用户**：控制面管理员 / 运营人员。
- **典型场景**
  - 给某个用户开通/关闭“在某个节点使用某种协议接入”的权限组合。
  - 对某个节点整体“全部禁用/全部启用”某些用户的接入。
  - 对某个协议整体“全部禁用/全部启用”某些用户的接入。

## 4) 需求列表（MUST/SHOULD/COULD）

### MUST

- UI 使用二维表格呈现：
  - 行：节点（来自 `GET /api/admin/nodes`）
  - 列：协议（至少覆盖现有 `AdminEndpointKind` 两种：`vless_reality_vision_tcp`、`ss2022_2022_blake3_aes_128_gcm`）
  - 单元格：该（节点、协议）组合的“允许接入”状态（勾选/未勾选）
- 行头与列头可批量操作（左上角为全局批量）：
  - 批量操作不提供“反选”
  - 批量操作的行为固定为：**若存在任意已选则清空，否则全选**
- 可视化反馈清晰：
  - 头部单元格能表达“该组当前是全选/全不选/部分选”（但点击行为仍遵循 MUST 的优先级规则）
  - 表格在节点行较多时可纵向滚动，并保持行头/列头可辨识（例如 sticky header/first column）
- 保存口径（实现阶段落地）：
  - 支持“先编辑、后保存”的工作流（一次性应用差异），避免每点一次就立即触发多次写操作
  - 保存失败可重试，并能看到失败原因（HTTP status/code/message）

### SHOULD

- 当某个（协议、节点）组合无法映射到唯一接入点时，单元格给出明确状态与引导（例如：无可用 endpoint / 存在多个 endpoint 需要选择）。
- 提供“筛选/折叠”能力以应对大量节点（例如按 node_name 搜索、仅显示可用列等）。
- 显示“已选数量 / 总数”的小统计，帮助确认批量操作结果。

### COULD

- 提供“预览将要新增/删除的 Grants”列表（diff view），用于保存前确认。
- 如果数据量变大，提供更高效的数据拉取方式（例如按 user 过滤 grants，而非拉取全量）。

## 5) 接口清单与契约（Inputs/Outputs/Errors）

本计划以“管理端 UI + 既有管理 API”为主，必要时补充少量 HTTP API 以提升可用性与性能。

### 接口清单（Inventory）

| 接口（Name）                                  | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）     | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                                       |
| --------------------------------------------- | ------------ | ------------- | -------------- | ---------------------------- | --------------- | ------------------- | --------------------------------------------------- |
| Grant 接入点二维表组件（`GrantAccessMatrix`） | UI Component | internal      | New            | ./contracts/ui-components.md | web             | 管理端页面          | 草图见 `./assets/grant-access-matrix-wireframe.svg` |
| Admin APIs：nodes/endpoints/grants            | HTTP API     | internal      | None           | ./contracts/http-apis.md     | xp              | web                 | 计划阶段仅复述依赖口径                              |
| （可选）Grants 查询过滤（按 user）            | HTTP API     | internal      | Modify         | ./contracts/http-apis.md     | xp              | web                 | 仅在数据量需要时启用                                |
| （可选）批量应用矩阵变更                      | HTTP API     | internal      | New            | ./contracts/http-apis.md     | xp              | web                 | 也可用多次 create/delete 替代                       |

## 6) 约束与风险

- 约束：现有 `Grant` 结构是 `grant -> endpoint_id`，而矩阵的维度是（protocol, node）。需要定义“单元格 ↔ endpoint ↔ grant”的确定映射，否则会出现“一个格子对应多个 endpoint/grant”的歧义。
- 风险：如果同一（node, kind）存在多个 endpoint，矩阵的“一格”可能不足以表达选择；需要额外设计（见开放问题）。
- 风险：如果实现阶段用“拉取全量 grants 再 client 过滤”，当 grants 数量增长时可能变慢；可能需要（可选）API 过滤或分页。

## 7) 验收标准（Acceptance Criteria）

### 表格形状与交互（核心）

- Given 系统存在至少 1 个 Node，且协议集合已知，
  When 打开 Grant 接入点配置界面，
  Then 页面以二维表格展示行=节点、列=协议，
  And 每个单元格均可表达“允许/不允许”（或明确的不可用原因）。

- Given 任意一行（某个节点）中存在至少 1 个已选单元格，
  When 点击该行行头的批量开关，
  Then 该行所有单元格变为“未选”（取消选择优先）。

- Given 任意一行（某个节点）中所有单元格均为未选，
  When 点击该行行头的批量开关，
  Then 该行所有单元格变为“已选”（全选）。

- Given 任意一列（某个协议）中存在至少 1 个已选单元格，
  When 点击该列列头的批量开关，
  Then 该列所有单元格变为“未选”（取消选择优先）。

- Given 任意一列（某个协议）中所有单元格均为未选，
  When 点击该列列头的批量开关，
  Then 该列所有单元格变为“已选”（全选）。

- Given 全表存在至少 1 个已选单元格，
  When 点击左上角（行列相交）的全局批量开关，
  Then 全表所有单元格变为“未选”（取消选择优先）。

- Given 全表所有单元格均为未选，
  When 点击左上角（行列相交）的全局批量开关，
  Then 全表所有单元格变为“已选”（全选）。

### 保存与一致性（实现阶段落地）

- Given 对矩阵做出若干选择变更但尚未保存，
  When 点击“Save / Apply changes”，
  Then 系统按差异创建/删除对应的 grants（或等价行为），
  And 保存成功后矩阵状态与后端数据一致，
  And 保存失败时显示可理解的错误信息并允许重试。

## 8) 开放问题（需要主人回答）

1. 这个二维表格是放在 **User 详情页**里（针对单个用户配置接入点），还是替换/增强 **Grant 新建页**（一次创建多个 grants）？你更希望入口在哪里？
2. “允许的接入点”在数据层面是否仍然落到“按 `endpoint_id` 创建多个 Grant”上？还是你希望未来变成“一个 Grant 里包含多个接入点”的新模型？
3. 对同一（node, protocol）如果存在 **多个 endpoints**（同 kind 同 node 多条），矩阵的一个格子应该如何处理：只允许选择“默认/主 endpoint”，还是格子里需要一个下拉选择具体 endpoint？
4. 矩阵的单元格是否需要显示更多信息（例如端口 `port`、tag、该格子对应的 grant_id），还是只要“是否允许”即可？
5. 批量操作是否需要二次确认（例如“全局清空”）来防误触，还是通过“先编辑后保存”已足够？

## 9) 假设（需主人确认）

- 假设：协议维度等价于 `AdminEndpointKind`（当前两种），后续如新增 kind，矩阵能自然扩展为更多列。
- 假设：实现阶段以“先本地编辑、后一次性保存”的方式应用变更；批量操作只改变本地选择状态，不立即触发写请求。

## 非功能性验收 / 质量门槛（Quality Gates）

### UI 还原原则（重要）

- 实现阶段使用 daisyUI 组件库：复选框统一使用 daisyUI 自带 `checkbox`（含 `checked` / `indeterminate` / `disabled` 状态），**不照抄设计稿 SVG 里的复选框造型**。
- 设计稿对复选框的约束仅限于：对齐关系、尺寸等级、间距、状态语义与配色倾向；允许在不破坏整体布局的前提下，遵循 daisyUI 的默认细节（边框/圆角/阴影等）。

实现阶段完成后，至少运行（沿用仓库现有约定，不引入新工具）：

- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- 如涉及可视化组件：补充/更新 Storybook stories，并运行：
  - `cd web && bun run storybook`
  - `cd web && bun run test-storybook`

## 文档更新（Docs to Update）

- `docs/plan/0010:admin-ui-geek-redesign/PLAN.md`：如本计划落地后的 UI 信息架构与 0010 有冲突/重叠，需补充交叉引用与边界说明。
- （可选）`README.md` 或 `web/README.md`：补充“Grant 接入点矩阵”的入口与使用说明（仅在实现完成后再写）。

## 里程碑（Milestones）

- [x] M1: 交互与信息架构对齐（低保真草图确认）
- [x] M2: 冻结“单元格 ↔ endpoint ↔ grant”的映射与保存策略（含异常：无 endpoint / 多 endpoint）
- [x] M3: 高保真设计稿确认（Light/Dark，含表格细节）
- [x] M4: 实现与自测（UI + 必要的 API 支撑）

## 设计稿（Assets）

- 低保真草图（wireframe）：`./assets/grant-access-matrix-wireframe.svg`
- 高保真设计稿（light）：`./assets/grant-access-matrix-hifi-light.svg`
- 高保真设计稿（dark）：`./assets/grant-access-matrix-hifi-dark.svg`
