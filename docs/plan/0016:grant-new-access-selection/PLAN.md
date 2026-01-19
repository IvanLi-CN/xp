# Grant 新建页：接入点选择与创建交互优化（#0016）

## 状态

- Status: 已完成
- Created: 2026-01-18
- Last: 2026-01-19

## 1) 问题陈述

当前（目标）管理端交互以 **grant group** 为单位提交：前端不做“逐条 grant”操作，提交必须整组提交（见 #0017）。

Grant 新建页（`GrantNewPage`）使用“节点 × 协议”的二维矩阵选择接入点（Access points），并在页头提供创建入口。

现状：当选择 **2 个及以上**接入点时，页面会要求“只能选 1 个”才能提交（按钮禁用/表单报错）。这与矩阵提供的批量选择能力（行/列/全选）存在明显的认知冲突：管理员容易误以为可以一次性创建一个包含多个成员的 group，但实际被阻断。

本计划用于冻结该页面在“多选”场景下的产品口径，并在实现阶段落地一致的交互与反馈。

## 2) 目标 / 非目标

### Goals

- 明确并实现 Grant 新建页的“接入点选择 ↔ 创建”一致语义（尤其是多选场景）。
- 避免出现“按钮灰掉但不清楚为什么”的体验：提供明确的可操作提示与反馈。
- 使用 #0017 定义的 group-level Admin APIs（`/api/admin/grant-groups`），以“一次请求创建整组”完成写入；不引入逐条 grant 的交互接口。

### Non-goals

- 不在本计划中变更“配额周期/重置时间”的归属口径（由 #0017 定案；本页面只消费最终字段形状）。
- 不在计划阶段改动实现代码/迁移/依赖（本计划仅冻结口径与验收标准）。
- 不讨论更复杂的“批量编辑/批量撤销 grant”能力（如果需要，另开计划）。

## 3) 用户与场景

- **主要用户**：控制面管理员 / 运营人员。
- **典型场景**
  - 为一组用户在多个节点/协议上开通接入点，并希望一次提交形成一个 grant group（组名可后续改名）。
  - 误操作选中多个格子后，希望系统要么支持“批量创建”，要么在交互上从源头避免多选（但不能出现“看起来能多选、结果不能提交”的割裂）。

## 4) 需求列表（MUST）

> 注：当前默认方案是“支持批量创建”。若主人选择“强制单选”，本计划将改为 `重新设计（#<id>）` 或在本计划内重写范围与验收。

### MUST

- 必须生成/填写一个 `group_name`（全局唯一，作为 group identity 与 UI 展示名）：
  - 默认可自动生成（避免强迫命名阻塞操作），但允许管理员修改；
  - 校验建议与后端一致（kebab/snake slug）；提交遇到 `409` 时提示“组名已存在”并允许修改后重试。
- 选择数为 0 时：
  - `Create` 按钮禁用；
  - 页面给出明确提示：需要选择至少 1 个接入点。
- 选择数为 1 时（创建一个包含 1 个成员的 group）：
  - CTA 文案：`Create group`；
  - 成功后跳转到该 group 的详情页。
- 选择数大于 1 时（创建一个包含 N 个成员的 group）：
  - CTA 文案体现数量（例如 `Create group (N members)`），且按钮可点击；
  - 点击后一次性提交整组 payload，服务端原子创建该 group 与其成员集合；
  - 提交期间显示 loading，且避免重复提交（按钮与关键输入禁用）。
- 批量创建的结果反馈必须可操作：
  - 成功：toast 明确提示“Created group with N members.”，并跳转到 group 详情页。
  - 失败：toast 提示失败原因（如可读），并允许重试（不会产生部分提交）。
- 校验口径清晰一致：
  - `quota_limit_bytes` 必须是 `>= 0`；
  - 多选时不得出现“只能选 1 个”的旧错误提示。

## 5) 接口清单与契约（Inputs/Outputs/Errors）

本计划不新增/修改/删除接口；创建/更新使用 #0017 定义的 group-level API：

- `POST /api/admin/grant-groups`（一次性创建整组）
  - Request body 必须包含 `group_name`（见 #0017 `AdminGrantGroupCreateRequest`）

## 6) 约束与风险

- **性能/体验**：当 N 较大时，请求体会变大，且后端需要在一次写入中计算 diff/校验；需要定义可接受上限与 UI 反馈方式（进度/可取消与否）。
- **重复创建风险**：如果“创建 group”被重复触发，可能产生重复 group。实现阶段需要明确防重复策略（至少避免双击重复提交）。
- **错误可理解性**：批量失败时若只展示“失败”而无法定位到具体格子，会显著降低可用性。

## 7) 验收标准（Acceptance Criteria）

### 选择与 CTA

- Given 页面加载完成且已选择某个 user，
  When 未选择任何接入点，
  Then `Create` 按钮禁用且提示“需要选择至少 1 个接入点”。

- Given 已选择 1 个接入点，
  When 点击 `Create group`，
  Then 发送 1 次 `POST /api/admin/grant-groups` 请求且成功后跳转到该 group 详情页。

- Given 已选择 N>1 个接入点，
  When 点击 `Create group (N members)`，
  Then 发送 1 次 `POST /api/admin/grant-groups` 创建请求，
  And UI 在提交期间显示 loading 并阻止重复提交，
  And 全部成功时提示“Created group with N members.”并跳转到 group 详情页。

### 表单校验

- Given `quota_limit_bytes < 0`，
  When 用户提交，
  Then UI 阻止提交并显示明确错误信息。

## 8) 非功能性验收 / 质量门槛（Quality Gates）

实现阶段完成后，至少运行（沿用仓库现有约定，不引入新工具）：

- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- 如涉及组件交互补充/更新 Storybook stories，并运行：
  - `cd web && bun run storybook`
  - `cd web && bun run test-storybook`

### Testing（需要补齐的测试）

- Unit tests（Vitest）：覆盖“多选 → 生成 1 个 group create payload（含 members）+ 提交状态管理（成功/失败）”的核心逻辑（建议抽成纯函数以便测试）。
- Storybook：补充一个 story/场景展示批量创建的交互状态（至少：N>1 时 CTA 文案、提交中、失败提示）。

## 9) 文档更新（Docs to Update）

- None（如实现阶段引入新的后端接口或改变既有接口，将在本计划补充契约文档并更新本节）。

## 10) 里程碑（Milestones）

- [x] M1: Web：Grant 新建页多选 CTA 与提交流程（创建 1 个 group，N members）
- [x] M2: Web：创建结果反馈（成功/失败）与可重试
- [x] M3: Web：补齐测试（Vitest）与 Storybook 场景

## 11) 方案概述（Approach, high-level）

- UI：以“多选可批量创建”为默认口径，CTA 明确展示创建数量；单选场景保持“创建后跳详情”的现有体验。
- 请求：使用 `POST /api/admin/grant-groups` 一次性创建整组（#0017）；members 由矩阵选择结果生成。
- 错误：错误提示需要能定位到具体格子（node/protocol/endpoint）以便管理员修正与重试。

## 12) 风险与开放问题（Risks & Open Questions）

- 风险：
  - 单次请求的 payload 可能较大；需要定义上限与失败提示策略（避免“看起来卡住”）。
  - 是否允许创建时与既有 group 成员重复（去重/迁移策略）需要后端口径配合（#0017）。

- 开放问题：
  - None（关键口径已对齐至 #0017：多选创建 1 个组；冲突返回 409。）

## 13) 假设（已确认）

- 多选创建 1 个 group（N members），提交整组 payload。
- 创建成功后跳转到 group 详情页。
- 实现阶段由 #0017 提供 `grant-groups` API；本页面不再调用 `grants` API。

## 参考（References）

- 现状实现：`web/src/views/GrantNewPage.tsx`
- 相关既有计划：`docs/plan/0012:grant-access-matrix/PLAN.md`

## 设计稿（Assets）

- `./DESIGN.md`

## Change log

- 实现“多选创建 1 个 group（N members）”的提交流程，新增 `/api/admin/grant-groups` 后端接口与前端对接。
- 交互与反馈对齐：0/1/N 选择态 CTA、提交 loading、防重复提交、冲突（409）可重试。
