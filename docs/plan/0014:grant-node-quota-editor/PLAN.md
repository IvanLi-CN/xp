# Grant 按节点配额编辑（支持 GiB/MiB 智能解析）（#0014）

## 状态

- Status: 待实现
- Created: 2026-01-16
- Last: 2026-01-16

## 1) 问题陈述

我们需要在管理端实现“按节点限制流量”：**流量配额与协议无关，只与节点有关**。当前系统以 grant（绑定 `endpoint_id`）为主要配置单元，配额字段是 `Grant.quota_limit_bytes`（bytes，`u64`），天然更偏向“按 endpoint/grant”的粒度。

当同一节点允许多个协议时，如果仍按 grant 粒度分别计量/封禁，就会出现“每个协议各有一份配额”的错误效果；而我们需要的是“该节点下所有协议的总使用量共享同一个节点配额”。

补充事实（已核查）：`quota_limit_bytes` 在后端/持久化/HTTP API/前端 schema 口径一致，均为 **bytes（`u64`）**，持久化在 `state.json` 中为 JSON number。

## 2) 目标 / 非目标

### Goals

- 在 User details 页提供“节点 × 协议”的接入矩阵，并在**每个节点行头**提供该节点的“流量配额”编辑（对该节点所有协议生效）。
- 配额输入支持单位并智能解析：
  - 支持 `GiB` / `MiB`（及常见写法变体），大小写不敏感，允许空格；
  - 无单位默认 `MiB`；
  - 允许小数，换算 bytes 后四舍五入；
  - `GB/MB` 兼容按 `GiB/MiB`（二进制口径）解释。
- “只保留这一种流量设置方式”：管理端所有“配额编辑”入口收敛到 User details 的矩阵行头；其他页面仅展示或引导跳转。

### Non-goals

- 不扩展到 TiB/KB 等更多单位（除非主人明确要求）。
- 不引入更复杂的配额模型（例如按地区/按协议族拆分配额池），只做“每用户-每节点 1 个配额”的规则。
- 不在计划阶段改动实现代码/迁移/依赖（本计划仅冻结口径与契约）。

## 3) 用户与场景

- **主要用户**：控制面管理员 / 运营人员。
- **典型场景**
  - 为某个用户在某个节点设置配额（例如 `20GiB`），不关心使用的是 VLESS 还是 SS2022。
  - 同一节点同时启用多个协议时，希望该节点下的总使用量共享同一配额并统一触发封禁。

## 4) 需求列表（MUST/SHOULD/COULD）

### MUST

- 在用户详情页提供“Access & quota”区块：
  - 表格行=节点（Node），列=协议（Protocol）；
  - 单元格仍用于“是否允许该协议接入”与 endpoint 选择（沿用 0012 口径）。
- 节点行头提供节点配额编辑入口：
  - 展示态：显示 `Quota: <human>`（例如 `10 GiB`）；
  - 编辑态：输入框 + Apply/Cancel；
  - 提交：Enter 或 Apply；取消：Esc 或点击外部取消；取消不写入。
- 错误提示不影响布局：
  - 输入非法时错误提示以“输入框下方浮层（popover/tooltip）”呈现；
  - 显示/隐藏错误不应改变表格行高或推动其他元素位移（避免 layout shift）。
  - 浮层必须始终显示在输入框下方；当下方空间不足时必须先自动滚动以确保浮层完整可见。
  - 浮层容器尺寸需自适应文本内容（不写死宽度），并在长文本时允许换行（设置合理 `max-width`）。
- 节点配额对该节点所有协议生效：
  - UI 展示在节点行头（不拆到协议格子）；
  - 该节点下不同协议看到的是同一个配额值。
- 单位解析规则明确且可单元测试：
  - 支持 `GiB/MiB` 及常见变体；
  - 无单位默认 `MiB`；
  - 允许小数并四舍五入到 bytes；
  - `GB/MB` 兼容按 `GiB/MiB` 解释；
  - 空字符串非法；`0` 合法（语义：不做配额封禁）；
  - 结果超过 `Number.MAX_SAFE_INTEGER` 必须报错并阻止提交（避免精度丢失）。
- 节点维度限额语义必须正确（后端实现阶段落地）：
  - 同一节点下多个协议的使用量必须合并为“节点总使用量”；
  - 达到节点配额阈值时，对该节点下所有协议执行封禁/禁用（或等价限制动作）。

### SHOULD

- bytes → human 的格式化稳定一致（优先 GiB，其次 MiB；展示文本带空格 `10 GiB`）。
- 当历史数据存在“同一节点下不同 grant 的 `quota_limit_bytes` 不一致”时，UI 显示 `Mixed` 状态并引导统一（而不是静默选择某个值）。
- 写入失败时采用 toast + 单元格内错误提示双通道，降低误判。

### COULD

- 节点行头支持“一键统一该节点所有 grants 的配额”（当存在 Mixed 状态时）。
- 支持批量给多个节点设置相同配额（未来扩展，不是本计划必须）。

## 5) 接口清单与契约（Inputs/Outputs/Errors）

为避免在前端对多个 grants 进行 N 次 patch（且需要后端保证节点语义），建议引入一个“按用户-按节点”设置配额的 Admin API（由后端统一落地与执行语义）。

### 接口清单（Inventory）

| 接口（Name）                                | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）     | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）      |
| ------------------------------------------- | ------------ | ------------- | -------------- | ---------------------------- | --------------- | ------------------- | ------------------ |
| 节点配额矩阵 UI（`GrantAccessMatrix` 扩展） | UI Component | internal      | Modify         | ./contracts/ui-components.md | web             | 管理端              | 行头编辑节点配额   |
| Admin API：User node quotas（list/set）     | HTTP API     | internal      | New            | ./contracts/http-apis.md     | xp              | web                 | 统一后端语义与写入 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/ui-components.md](./contracts/ui-components.md)
- [contracts/http-apis.md](./contracts/http-apis.md)

## 6) 验收标准（Acceptance Criteria）

### 节点配额编辑体验（UI）

- Given 用户详情页的 Access & quota 表格已加载，
  When 用户在某节点行头输入 `1.5GiB` 并按 Enter（或点击 Apply），
  Then UI 提交更新请求并在成功后显示该节点配额为 `1.5 GiB`（或等价可读格式）。

- Given 用户在编辑态按 Esc 或点击外部区域，
  When 退出编辑态，
  Then UI 不发出写请求并恢复原值显示。

- Given 输入非法（例如空字符串、`abc`、负数、超出 safe integer），
  When 用户尝试提交，
  Then UI 阻止提交并显示错误提示。

### 节点限额语义（后端）

- Given 同一节点下启用了多个协议并产生使用量，
  When 该节点“所有协议合计”的总使用量达到节点配额阈值，
  Then 系统对该节点下所有协议执行封禁/禁用（或等价限制动作），
  And 管理端能观察到一致的 quota enforced 状态与原因。

### 单位解析（示例驱动，必须可单元测试）

以下示例中，`GiB = 2^30 bytes`，`MiB = 2^20 bytes`：

| 输入                                                 | 期望                                       |
| ---------------------------------------------------- | ------------------------------------------ |
| `10GiB` / `10 gib` / `10 GiByte` / `10 gibibyte`     | 解析为 `10 * 2^30` bytes                   |
| `512MiB` / `512 mib` / `512 MiByte` / `512 mebibyte` | 解析为 `512 * 2^20` bytes                  |
| `10  GiB`（前后空格）                                | 解析成功                                   |
| `10`（无单位）                                       | 解析为 `10 MiB`                            |
| `1.5GiB`                                             | 解析为 `round(1.5 * 2^30)` bytes           |
| `10GB` / `10MB`                                      | 兼容识别为 `10GiB` / `10MiB`               |
| `0` / `0MiB`                                         | 解析为 `0` bytes（系统语义：不做配额封禁） |
| ``（空字符串）                                       | 解析失败（必须输入数字）                   |
| `-1GiB`                                              | 解析失败（非负约束）                       |
| `abc` / `10XB`                                       | 解析失败（无法识别单位/数值）              |

## 7) 非功能性验收 / 质量门槛（Quality Gates）

实现阶段完成后，至少运行（沿用仓库现有约定，不引入新工具）：

- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- 涉及组件交互时补充/更新 Storybook stories，并运行：
  - `cd web && bun run storybook`
  - `cd web && bun run test-storybook`

### Testing（需要补齐的测试）

- Unit tests（Vitest）：为“单位解析函数”补齐覆盖（含大小写/空格/GB/MB 兼容/小数四舍五入/非法输入/边界值）。
- Storybook：更新 `GrantAccessMatrix` 相关 story，覆盖“行头配额编辑：展示态/编辑态/错误态/提交中”。

## 8) 文档更新（Docs to Update）

- `docs/plan/0012:grant-access-matrix/PLAN.md`：补充交叉引用（本计划在 0012 的矩阵上追加“按节点配额”能力，并移除“按协议格子配额”的设想）。

## 9) 里程碑（Milestones）

- [x] M1: 冻结“按节点配额（行头）编辑 + 提交动作 + 错误展示”的交互口径（含高保真设计稿与 UI 设计文档）
- [x] M2: 冻结“单位解析规则（含无单位/小数/GB/MB 兼容等歧义）”并补齐示例
- [ ] M3: 实现与自测（web + xp；含合计计量与封禁）

## 10) 方案概述（Approach, high-level）

- UI：矩阵仍按“节点 × 协议”展示接入，但配额只放在行头，明确“节点配额”的语义。
- Web：新增一个纯函数解析器 `string -> bytes | error`；展示层统一 human formatter。
- XP：引入“按用户-按节点”配额的存储与计算，并在 quota tick 中按节点合计使用量、统一封禁该节点下所有相关 grants。
- API：新增 admin API 作为唯一写入口（避免前端多次 patch 造成不一致）。

## 11) 风险与开放问题（Risks & Open Questions）

- 风险：
  - 现有数据模型以 grant 粒度为中心，按节点合计与封禁需要谨慎处理“多协议/多 endpoint/节点 owner”的边界与一致性。
  - JS `number` 的安全整数上限限制输入范围；需要友好的错误提示与上限策略。

- 需要决策的问题（请逐条回答）：
  1. 是否允许在“该节点当前未启用任何协议/未存在任何 grant”时预先设置节点配额？（若允许，必须有独立于 grants 的 node quota 存储）
  2. 对历史数据里“同一节点下不同 grants 的 `quota_limit_bytes` 不一致”，默认展示策略是否为 `Mixed` 并要求手动统一？

## 12) 假设（需主人确认）

- 假设：节点配额的语义是“该节点下所有协议的使用量合计共享同一配额”，达到阈值后统一封禁该节点下所有相关 grants。
- 假设：配额编辑入口只收敛到 User details 的矩阵行头；其他页面不再提供第二种配额编辑入口。

## 设计稿（Assets）

- UI 设计文档：`./DESIGN.md`
- 设计稿（light）：`./assets/grant-node-quota-editor-hifi-light.svg`
- 设计稿（dark）：`./assets/grant-node-quota-editor-hifi-dark.svg`
