# 控制面 Web UI 重设计（Geek 风格 + Light/Dark）（#0010）

## 状态

- Status: 待验收
- Created: 2026-01-13
- Last: 2026-01-14

## 1) 问题陈述

当前控制面 Web UI 功能可用，但“信息层级 / 视觉一致性 / 密度与可读性 / 操作效率”明显不足，整体更像临时页面而非可长期维护的内部运维面板。需要在不改变后端能力与核心交互流程的前提下，重做一套更专业、业内真实的 Geek 风格界面，并提供 Light/Dark 两套风格。

## 2) 目标 / 非目标

### Goals

- 建立统一的 App Shell（顶部状态栏 + 侧边导航 + 内容区），覆盖现有所有页面并可持续扩展。
- 提供 **Light / Dark** 两套主题：可切换、可持久化、默认跟随系统（可配置）。
- 统一视觉语言：排版层级、间距、卡片/表格/表单/提示/加载态/空态的规范与组件化。
- 全站图标统一来自 **Iconify**（建议选择 `tabler:` 作为主图标集合），并给出可复用的 Icon 组件约定。
- 使用 **daisyUI** 作为组件与主题实现基础（不引入“自研 UI 框架”）。

### Non-goals

- 不新增/修改后端 API、业务权限模型、数据结构与校验规则（仅 UI 表达与交互一致性改进）。
- 不把 UI“赛博/黑客化”：拒绝霓虹绿、终端雨、贴图电路板等外行刻板印象。
- 不做信息架构的产品重构（如新增复杂的多级菜单/多租户/审计中心等），除非主人明确要求。

## 3) 用户与场景

- **主要用户**：控制面维护者（cluster operator / admin）。
- **高频场景**
  - 快速确认系统状态（health、leader、term、告警概览）。
  - 管理资源列表（Nodes / Endpoints / Users / Grants）的检索、查看详情、创建与复制信息。
  - 出错时快速定位（明确错误来源、可重试、可复制错误详情、保留上下文）。

## 4) 需求列表（MUST/SHOULD/COULD）

### MUST

- 全站具备一致的 App Shell：顶部状态栏（全局状态 + 快捷操作）+ 侧边导航（含图标与当前路由高亮）+ 主内容区域。
- 支持 **Light / Dark** 两套主题，并可在 UI 内切换；刷新后保持（localStorage 或等价方案）；默认行为可配置为“跟随系统”。
- 图标来源统一为 **Iconify**；图标命名约定与渲染方式在契约中冻结（避免后续随意混用库/图标集）。
- 使用 daisyUI 组件与主题能力实现（例如 `drawer/navbar/menu/table/card/badge/alert/toast/modal` 等），并输出可复用组件清单。
- 覆盖现有页面：`/login`、Dashboard(`/`)、Nodes、Endpoints、Users、Grants 及其 Details/New 页面：至少完成“布局与主要内容区”一致化。
- 信息密度与可读性：ID/token/URL 使用等宽字体；表格列对齐、可复制字段的呈现一致；移动端不溢出/不遮挡。
- 提供“密度（density）”切换（comfortable/compact），并持久化（适用于表格/表单/列表）。

### SHOULD

- 全局搜索/命令面板（Ctrl/⌘+K）作为高级用户入口（可先做 UI 占位与交互契约，后续实现逐步补齐）。
- 更专业的状态呈现：Health/Role/Leader/Term/Alerts 统一为状态徽标（badge）与颜色语义，并为 Dark/Light 保证对比度。

### COULD

- 为关键列表提供列显示/排序的“轻量偏好”持久化（不改变数据语义）。
- Dashboard 增加“最近操作/事件”区（若后端无事件接口则仅占位，等待后续计划）。

## 5) 接口清单与契约（Inputs/Outputs/Errors）

本计划的“接口”主要是 **前端内部接口**（UI Component / Config）。不涉及后端 HTTP API 的变更。

### 接口清单（Inventory）

| 接口（Name）                         | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）     | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                   |
| ------------------------------------ | ------------ | ------------- | -------------- | ---------------------------- | --------------- | ------------------- | ------------------------------- |
| `UiThemeConfig`                      | Config       | internal      | New            | ./contracts/config.md        | web             | `AppLayout` / 全站  | theme + follow-system + persist |
| `UiDensityConfig`                    | Config       | internal      | New            | ./contracts/config.md        | web             | Table/Form          | compact vs comfortable          |
| `Icon`                               | UI Component | internal      | New            | ./contracts/ui-components.md | web             | 全站                | Iconify-only                    |
| `AppShell`                           | UI Component | internal      | Modify         | ./contracts/ui-components.md | web             | 全站                | 替换现有 `AppLayout` 结构       |
| `PageHeader`                         | UI Component | internal      | New            | ./contracts/ui-components.md | web             | views/*             | 标题/描述/操作区统一            |
| `DataTable`（或 `ResourceTable` v2） | UI Component | internal      | Modify         | ./contracts/ui-components.md | web             | list pages          | 列对齐/空态/加载态一致          |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/config.md](./contracts/config.md)
- [contracts/ui-components.md](./contracts/ui-components.md)

## 6) 约束与风险

### 约束

- 只允许使用现有技术栈：Vite + React + TanStack Router/Query + Tailwind + daisyUI。
- 主题需兼容 daisyUI v5（当前 `web` 依赖已包含 daisyUI）。
- 图标必须来自 Iconify（实现阶段会新增 Iconify 依赖；计划阶段只冻结约定）。

### 风险

- “Geek 风格”定义容易发散：若无明确视觉基准，容易出现偏主观的反复调整。
- 仅靠 daisyUI 内置主题可能无法达到“高保真一致性”；可能需要自定义 `xp-light` / `xp-dark` 两个主题并维护 token。
- 表格/表单密度提升若处理不当，会在移动端造成可用性下降（需要明确响应式策略）。

## 7) 验收标准（Given/When/Then + 边界/异常）

### 主题与外观

- Given 用户首次打开 UI 且系统为 Light
  When 进入任意页面
  Then 默认使用 Light 主题（或按“跟随系统”配置策略生效），页面无样式闪烁（FOUC 不明显）。
- Given 用户在顶部状态栏切换到 Dark
  When 刷新页面并再次进入
  Then 仍保持 Dark，且主要组件（导航/卡片/表格/表单/提示）均符合 Dark 对比度要求。

### 图标与一致性

- Given 任意导航项与页面关键操作（新增/刷新/返回/复制/删除等）
  When 查看图标渲染
  Then 图标来源均为 Iconify，并符合约定的 icon set 与命名规则；不出现混用 SVG 内联/emoji/不同图标库。

### App Shell 与页面覆盖

- Given 已登录并进入 `/`
  When 在侧边导航切换 Nodes/Endpoints/Users/Grants
  Then 顶部状态栏与侧边导航保持一致，主内容区域按统一的 `PageHeader` + `Content` 模板渲染。
- Given 进入任意 Details/New 页面
  When 返回或切换路由
  Then 布局不跳动，标题/面包屑/主要操作区位置一致。

### 状态与异常

- Given 后端不可达或接口返回错误
  When 任意页面请求失败
  Then 错误信息具备统一的可读格式、可重试入口与可复制的错误详情（便于排障）。
- Given 列表为空
  When 进入列表页
  Then 空态具有一致的说明与主要操作（如 New / Refresh）。
- Given 用户在顶部状态栏或设置区切换 density 为 `compact`
  When 刷新页面并浏览列表/表单
  Then 密度偏好被持久化且生效（行高/间距/按钮尺寸按约定变化），并可切回 `comfortable`。

## 8) 开放问题（需要主人回答）

None

## 9) 假设（需主人确认）

- 已确认：本计划高保真设计图作为最终视觉基准。
- 已确认：主题切换默认策略为 `system`（跟随系统）。
- 已确认：图标默认限定 Iconify `tabler:`；后续如需扩展需通过计划更新冻结新规则。
- 已确认：密度切换为 MUST（`comfortable` / `compact`，并持久化）。
- assumption：不更改后端 API；页面字段与交互流程（CRUD 与复制/刷新/重试）保持现有语义，仅重做布局与视觉规范。
- assumption：实现阶段允许新增 Iconify 的前端依赖（例如 `@iconify/react`）以满足“所有图标来自 iconify”。

## 高保真 UI 设计图（Light/Dark）

> 说明：以下为“可落地到 daisyUI”的高保真布局稿，用于冻结信息层级、组件形态与交互入口；实现阶段以此为视觉基准，允许在不改变信息架构的前提下做小幅对齐。

- 设计稿用图标集：Iconify `tabler:`（Tabler Icons）
- 设计稿图标清单（Iconify name → 用途）
  - `tabler:layout-dashboard` → Sidebar / Dashboard
  - `tabler:server` → Sidebar / Nodes
  - `tabler:plug` → Sidebar / Endpoints
  - `tabler:users` → Sidebar / Users
  - `tabler:key` → Sidebar / Grants
    -（实现阶段补齐：Top bar / Theme toggle、Search、Logout、Copy、Refresh、New、Back 等动作图标，统一在契约文档中冻结）

### 页面一览（Routes）

| Route                    | Page             | Light                                                          | Dark                                                         |
| ------------------------ | ---------------- | -------------------------------------------------------------- | ------------------------------------------------------------ |
| `/login`                 | Login            | ![Login light](./assets/login-light.svg)                       | ![Login dark](./assets/login-dark.svg)                       |
| `/`                      | Dashboard        | ![Dashboard light](./assets/dashboard-light.svg)               | ![Dashboard dark](./assets/dashboard-dark.svg)               |
| `/nodes`                 | Nodes list       | ![Nodes light](./assets/nodes-light.svg)                       | ![Nodes dark](./assets/nodes-dark.svg)                       |
| `/nodes/$nodeId`         | Node details     | ![Node details light](./assets/node-details-light.svg)         | ![Node details dark](./assets/node-details-dark.svg)         |
| `/endpoints`             | Endpoints list   | ![Endpoints light](./assets/endpoints-light.svg)               | ![Endpoints dark](./assets/endpoints-dark.svg)               |
| `/endpoints/new`         | New endpoint     | ![Endpoint new light](./assets/endpoint-new-light.svg)         | ![Endpoint new dark](./assets/endpoint-new-dark.svg)         |
| `/endpoints/$endpointId` | Endpoint details | ![Endpoint details light](./assets/endpoint-details-light.svg) | ![Endpoint details dark](./assets/endpoint-details-dark.svg) |
| `/users`                 | Users list       | ![Users light](./assets/users-light.svg)                       | ![Users dark](./assets/users-dark.svg)                       |
| `/users/new`             | New user         | ![User new light](./assets/user-new-light.svg)                 | ![User new dark](./assets/user-new-dark.svg)                 |
| `/users/$userId`         | User details     | ![User details light](./assets/user-details-light.svg)         | ![User details dark](./assets/user-details-dark.svg)         |
| `/grants`                | Grants list      | ![Grants light](./assets/grants-light.svg)                     | ![Grants dark](./assets/grants-dark.svg)                     |
| `/grants/new`            | New grant        | ![Grant new light](./assets/grant-new-light.svg)               | ![Grant new dark](./assets/grant-new-dark.svg)               |
| `/grants/$grantId`       | Grant details    | ![Grant details light](./assets/grant-details-light.svg)       | ![Grant details dark](./assets/grant-details-dark.svg)       |

### App shell 基准图

- App shell（Light）：![App shell light](./assets/app-shell-light.svg)
- App shell（Dark）：![App shell dark](./assets/app-shell-dark.svg)

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests（Vitest）：新增/更新核心 UI 组件（`Icon`、`PageHeader`、`DataTable`）的渲染与交互测试。
- E2E（Playwright）：覆盖至少 1 条关键路径：登录 → 进入 Dashboard → 进入某个列表页 → 打开详情 → 切换主题 → 刷新保持。

### UI / Storybook

- 新增 stories：`Icon`、`PageHeader`、`DataTable`、`AppShell` 的 Light/Dark 变体。
- `test-storybook` 必须通过；如引入视觉回归基线（若仓库已有流程）则按既有约定更新。

### Quality checks

- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
  -（如启用）`cd web && bun run test:e2e` / `cd web && bun run test-storybook`

## 文档更新（Docs to Update）

- `docs/desgin/tech-selection.md`：补充 UI 技术选择口径（daisyUI v5 + Iconify + 主题策略）。
- `docs/desgin/quality.md`：补充前端 UI 的质量门槛与回归策略（Storybook / test-storybook / E2E 覆盖范围）。

## 里程碑（Milestones）

- [x] M1: 冻结视觉规范（主题 token、图标集、密度策略、App Shell 信息层级）
- [x] M2: App Shell 落地（导航/顶部状态栏/主题切换/响应式）
- [x] M3: 组件落地（Icon、PageHeader、DataTable、表单区块模板）
- [x] M4: 覆盖所有页面（list/details/new/login/dashboard）
- [x] M5: 质量门槛补齐（Storybook + Vitest + E2E）

## 方案概述（Approach, high-level）

- 以 “App Shell + 组件规范” 为中心推进：先把容器与基础组件稳定下来，再逐页迁移。
- 主题采用 daisyUI theme：优先定义 `xp-light` / `xp-dark` 两套主题 token，并在 Tailwind/daisyUI 层统一（减少页面级手写颜色）。
- 图标统一走一个 `Icon` 组件，禁止页面直接引入随机 SVG；Icon 名称使用 `set:name` 形式并集中管理（便于审计与替换）。
- 列表/表单/详情统一布局模板：避免每个页面各写各的 spacing/typography。

## 风险与开放问题（Risks & Open Questions）

- 风险：视觉基准不明确会导致反复；需要主人尽快确认“以本文设计图为基准”或给出参考产品。
- 风险：密度/响应式与可访问性需要权衡；如密度作为 MUST，需接受更多 UI 细节工作量。
- 开放问题：见「8) 开放问题」。

## 参考（References）

- daisyUI theme / components
- Iconify icons（建议：Tabler Icons 集）
