# XP Web DaisyUI -> shadcn/ui 全量迁移（#d4kex）

## 状态

- Status: 已完成
- Created: 2026-03-09
- Last: 2026-03-09

## 背景 / 问题陈述

- 当前 `web/` UI 基座是 DaisyUI v5 + 页面级手写 class 组合，缺少本地组件库边界，弹窗、按钮、表单、通知与表格存在重复骨架与重复状态语义。
- 控制面已经积累大量可复用界面片段，继续保留 DaisyUI token 会增加主题、密度、Storybook 文档与长期维护成本。
- 需要在不改变后端 API、路由、认证与信息架构的前提下，把 UI 基础设施切换到 shadcn/ui，并把 Storybook stories/docs 变成硬门禁。

## 目标 / 非目标

### Goals

- 在单一实现分支内移除 DaisyUI，并把 `web/` 切换到 `shadcn/ui + Tailwind CSS v4 + Radix/Sonner` 本地组件体系。
- 保留 `xp-light` / `xp-dark`、`xp_ui_theme` / `xp_ui_density`、现有路由结构、Iconify 入口与当前页面信息架构。
- 完整表单统一接入 `react-hook-form + zod + @hookform/resolvers`，共享控件与高频交互统一到 app wrapper / shadcn primitives。
- 所有通用视觉组件都必须拥有独立 Storybook stories 与 docs 页面，并通过 `test-storybook`。
- 交付包含代码、文档、验证、PR、CI 与 review-loop 收敛。

### Non-goals

- 不修改后端 HTTP/SSE 协议、TanStack Router 路由路径、管理员令牌模型或业务数据结构。
- 不借机重做页面信息架构或新增功能域。
- 不保留 DaisyUI 与 shadcn/ui 的长期双栈共存状态。

## 范围（Scope）

### In scope

- `web/` 依赖、Vite/Storybook/Tailwind 配置、主题 token、密度 token、全局样式基线。
- `src/components/ui/*` 本地组件库与 app 级共享组件层。
- 登录页、列表页、详情页、新建页、配置页、对话框、菜单、移动端导航、命令面板占位。
- Storybook、Vitest、Playwright、Rust 嵌入式 web 产物回归验证。
- `docs/desgin/tech-selection.md` 与本规格的实施状态同步。

### Out of scope

- 后端 API 或 Rust 服务行为变更。
- 设计风格的产品级重做；仅允许为适配 shadcn/ui 做必要结构调整。
- 历史 `docs/plan/0010:admin-ui-geek-redesign/PLAN.md` 的回写。

## 需求（Requirements）

### MUST

- 合并态不再包含 `daisyui` 依赖、插件、主题配置、补丁样式或 DaisyUI class-token。
- 新增 `components.json`、`@/*` alias、`src/lib/utils.ts` 与 shadcn 本地组件目录。
- `UiPrefs` 继续持久化当前 theme/density key，并同时驱动 light/dark 与 comfortable/compact。
- `Button`、`ConfirmDialog`、`ToastProvider/useToast`、`DataTable`、`PageState`、`PageHeader`、`AppShell` 等共享组件保留上层语义，但内部全部切换到 shadcn/Radix/Sonner。
- 完整表单迁到 RHF+Zod，并保持 payload、loading、错误展示与成功后导航语义不变。
- 所有通用组件必须具备独立 stories 与 docs 页面，docs 至少包含用途、主要 props/variants、边界态与 theme/density 表现。
- 本地验证矩阵必须覆盖 `bun run lint`、`typecheck`、`test`、`build`、`test-storybook`、`test:e2e` 与仓库级 `cargo test`。

### SHOULD

- Storybook docs 默认使用 Autodocs，复杂组件再补充 MDX 或额外说明。
- 高定制组件尽量挂靠现有 shadcn primitives，减少页面内直接拼装低层交互骨架。
- 视觉口径尽量维持现有 `xp-light/xp-dark` 与当前信息层级，降低迁移带来的视觉波动。

### COULD

- 为 Storybook 补充更明确的主题/密度 toolbar 文档说明。
- 为复杂组件增加更细的交互型故事用例。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 进入登录页、Dashboard、Nodes、Endpoints、Users、Quota Policy、Service Config、Reality Domains、IP Geo DB 等页面时，所有常规按钮、卡片、表格、表单、对话框、菜单、徽标和提示都通过 shadcn primitives 或 app wrapper 渲染。
- 用户切换 light/dark、comfortable/compact 后，主题与密度仍在刷新后保留，并在 Storybook 与真实页面上表现一致。
- 所有完整表单使用统一的 RHF+Zod 校验模式，错误状态、禁用状态、提交 loading 与后台错误反馈在视觉和交互上保持一致。
- 所有通用组件在 Storybook 中既能单独浏览，也能从 Docs 页面看到 props、状态、使用说明与主题/密度差异。
- `web/dist` 仍可被 Rust 二进制嵌入，前端构建链与 CI job 名称不变。

### Edge cases / errors

- 原本依赖原生 `<dialog>` 的交互必须迁到 `Dialog` / `AlertDialog` / `Sheet`，并保持 ESC/关闭按钮/遮罩关闭语义合理。
- 高定制组件如果不适合完全替换为官方模板，允许保留业务逻辑与布局，但不得继续依赖 DaisyUI token。
- 若 Storybook docs 无法从纯 Autodocs 清晰表达复杂交互，必须补充额外 docs 内容，而不是放弃 docs 页面。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                      | 类型（Kind）           | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers）                  | 备注（Notes）                                            |
| --------------------------------- | ---------------------- | ------------- | -------------- | ------------------------ | --------------- | ------------------------------------ | -------------------------------------------------------- |
| `UiThemeConfig`                   | internal UI contract   | internal      | Modify         | None                     | web             | `UiPrefsProvider`, Storybook         | 切到 shadcn CSS variables + dark class，保留 storage key |
| `UiDensityConfig`                 | internal UI contract   | internal      | Modify         | None                     | web             | `UiPrefsProvider`, shared components | 保留 comfortable/compact 语义                            |
| `Component stories/docs contract` | internal docs contract | internal      | New            | None                     | web             | Storybook, reviewers                 | 所有通用组件必须提供 stories + docs                      |
| `Form composition contract`       | internal UI contract   | internal      | Modify         | None                     | web             | 所有表单页                           | 统一 RHF + Zod + shadcn Form                             |

### 契约文档（按 Kind 拆分）

- None

## 验收标准（Acceptance Criteria）

- Given 当前仓库进入 `web/`
  When 检查 `package.json`、Vite/Tailwind/Storybook 配置与 `src/styles.css`
  Then 不再出现 DaisyUI 依赖、插件、主题配置或 DaisyUI 补丁样式，并存在 shadcn 初始化产物与 Tailwind v4 基线。
- Given 用户在真实页面切换 `xp_ui_theme` 与 `xp_ui_density`
  When 刷新后再次进入任意主要页面
  Then 主题与密度偏好仍保留，且 shell、表格、表单、对话框与菜单表现一致。
- Given 用户访问任一完整表单页面
  When 输入非法值、提交、修正并再次提交
  Then 校验错误、禁用状态、loading 与成功/失败反馈统一且不改变原有业务 payload。
- Given 维护者打开 Storybook
  When 浏览任一通用组件
  Then 组件拥有独立 stories 与 docs 页面，且 docs 能展示用途、关键 props/variants、边界态与 theme/density 差异。
- Given 仓库执行前端与嵌入式 web 回归命令
  When 验证矩阵跑完
  Then `lint/typecheck/test/build/test-storybook/test:e2e/cargo test` 全部通过。

## 实现前置条件（Definition of Ready / Preconditions）

- 迁移目标、非目标、范围与交付流程已冻结。
- Tailwind v4、一次性切换、保留现有产品视觉、RHF+Zod 标准化、Storybook docs 硬门禁这些关键决策已锁定。
- 现有 DaisyUI 使用面与 Storybook 覆盖面已盘点，可据此分配并行实施边界。
- Git 分支、PR、CI、review-loop 快车道策略已明确。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 更新共享组件、表单、主题/密度与高风险交互的 Vitest 用例。
- Integration tests: 关键 CRUD / 编辑流程保持可用，必要时补充交互断言。
- E2E tests (if applicable): 保持现有 Playwright 路径通过，并修正受 UI 迁移影响的选择器。

### UI / Storybook (if applicable)

- Stories to add/update: 所有通用组件与高风险复合组件。
- Visual regression baseline changes (if any): 由 Storybook test-runner 覆盖；若需要 PR 证据图，放入 `./assets/` 并维护本规格的 `## Visual Evidence (PR)`。

### Quality checks

- Lint / typecheck / formatting: 沿用仓库现有 `bun run lint`、`bun run typecheck`、`bun run test`、`bun run build`、`bun run test-storybook`、`bun run test:e2e`、`cargo test`。

## 文档更新（Docs to Update）

- `docs/desgin/tech-selection.md`: 将 UI 技术栈从 DaisyUI 更新为 shadcn/ui + Tailwind v4 + RHF/Sonner/Radix 基线。
- `docs/specs/README.md`: 新增本规格并在实施推进时同步状态。
- `docs/specs/d4kex-shadcn-ui-full-migration/SPEC.md`: 跟踪实施进度、里程碑与最终证据。

## 计划资产（Plan assets）

- Directory: `docs/specs/d4kex-shadcn-ui-full-migration/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- PR visual evidence source: maintain `## Visual Evidence (PR)` in this spec when PR screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

## Visual Evidence (PR)

- 当前未生成真实 PR 截图；遵守授权规则，待主人明确同意后再补。

## 资产晋升（Asset promotion）

- None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新规格、索引与技术文档同步到 shadcn/ui 迁移口径。
- [x] M2: `web/` 基础设施完成 Tailwind v4 + shadcn 初始化产物切换，并移除 DaisyUI。
- [x] M3: 共享组件、通用交互与页面表单全部迁到 shadcn/ui / RHF+Zod。
- [x] M4: Storybook stories/docs、自动化测试、浏览器验证与快车道 PR 收敛完成。

## 方案概述（Approach, high-level）

- 先冻结规格与文档口径，再切换前端基础设施，随后把共享组件层作为页面迁移支点。
- 页面迁移优先复用 app wrapper 与 shadcn primitives，减少页面内重复交互骨架。
- Storybook 与测试在迁移过程中同步更新，避免最后集中补漏。
- 快车道以本地主代理单写入为主，远端 PR/CI/review-loop 按仓库既有流程收敛。

## 实施结果（Implementation result）

- `web/` 已移除 DaisyUI 依赖、Tailwind 插件与页面层 Daisy token；样式基线切到 Tailwind CSS v4 + shadcn/ui primitives。
- `UiPrefs` 继续持久化 `xp_ui_theme` / `xp_ui_density`，并同步驱动 `<html data-theme>`、`data-density` 与 `dark` class。
- 登录、用户、节点、配额、服务配置、Reality domains、IP Geo DB 等表单与关键界面已迁到 shadcn wrapper / RHF + Zod。
- Storybook 已覆盖现有通用组件与 `src/components/ui/*` 基础件，Autodocs 为默认 docs 路径。
- 验证通过：`cd web && bun run lint`、`bun run typecheck`、`bun run test`、`bun run build`、`bun run build-storybook`、`bun run test-storybook`、`E2E_BASE_URL=http://127.0.0.1:60180 bun run test:e2e`、`cargo test`。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：一次性切换涉及页面与测试面较广，容易出现选择器漂移、布局抖动或 Storybook 覆盖遗漏。
- 风险：Tailwind v4 与 Storybook/Vite 的组合若配置不完整，可能导致样式在 stories 与生产构建表现不一致。
- 需要决策的问题：None。
- 假设（需主人确认）：若 PR 需要视觉证据图，主人会在提交前对图片 push 授权给出明确确认。

## 变更记录（Change log）

- 2026-03-09: 创建规格并冻结 DaisyUI -> shadcn/ui 全量迁移范围、Tailwind v4、RHF+Zod、Storybook docs 硬门禁与快车道交付口径。
- 2026-03-09: 完成 DaisyUI 依赖与 token 清理，落地 shadcn/ui primitives、Tailwind v4、UiPrefs dark class 同步、RHF+Zod 表单统一与 Sonner/AlertDialog/Dialog/Sheet 迁移。
- 2026-03-09: 为通用组件与 `src/components/ui/*` 基础件补齐 Storybook stories/docs，修复 Storybook test-runner 超时门禁与 E2E 选择器漂移，完成 `lint`/`typecheck`/`test`/`build`/`build-storybook`/`test-storybook`/`test:e2e`/`cargo test` 验证。

## 参考（References）

- `docs/desgin/tech-selection.md`
- `docs/specs/README.md`
- `docs/plan/0010:admin-ui-geek-redesign/PLAN.md`
