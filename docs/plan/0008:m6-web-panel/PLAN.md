# Milestone 6 · Web 面板（基础功能完整：CRUD）（#0008）

## 状态

- Status: 已完成
- Created: 2025-12-22
- Last: 2025-12-22

## 背景 / 问题陈述

本计划由旧 planning/spec 文档迁移归档；主人已确认该计划对应功能**已实现**。

## 目标 / 非目标

详见下方“原始输入”中的相关章节（例如“背景与目标”“范围与非目标”等）。

## 范围（Scope）

详见下方“原始输入”。

## 需求（Requirements）

详见下方“原始输入”。

## 接口契约（Interfaces & Contracts）

详见下方“原始输入”（本计划为迁移归档，不在此额外新增契约文档）。

## 验收标准（Acceptance Criteria）

详见下方“原始输入”中的 DoD/验收清单/验收点等章节（如有）。

## 里程碑（Milestones）

- [x] 登录：输入 `admin_token` 并保存到浏览器本地（localStorage），提供退出/清除能力。
- [x] 全局反馈：统一的错误提示（可读的后端 error.code/message）、加载态与空态组件。
- [x] 危险操作确认：删除、重置 token、rotate shortId、禁用授权等操作需要二次确认。
- [x] Nodes：
- [x] Endpoints：列表 + 详情；创建/更新/删除；VLESS 端点支持 rotate shortId。
- [x] Users：列表 + 详情；创建/更新/删除；重置订阅 token。
- [x] Grants：列表 + 详情；创建/更新/删除；启用/禁用与配额/周期策略修改；可编辑 `note`；展示 credentials；展示 usage（不可用时显示 N/A）。
- [x] 在 User 详情中提供订阅入口：raw/base64/clash 的一键复制与格式切换。
- [x] 快速校验：对订阅请求做最小校验（HTTP 200 + 非空 + 基本格式），并提供可读错误。
- [x] Storybook：覆盖新增组件的典型/异常状态（loading/empty/error/disabled）。
- [x] Playwright：覆盖关键路径 E2E（登录、资源列表可见、创建/删除最小闭环等）。

## 方案概述（Approach, high-level）

详见下方“原始输入”。

## 风险与开放问题（Risks & Open Questions）

- None noted in source.

## 参考（References）

- `docs/desgin/README.md`

## 原始输入（迁移前版本）

# Milestone 6 · Web 面板（基础功能完整：CRUD）— 需求与概要设计

> 对齐计划：`docs/plan/README.md` 的 **Milestone 6**。\
> 参考：`docs/desgin/api.md` / `docs/desgin/subscription.md` / `docs/desgin/quality.md` / `docs/desgin/workflows.md`

## 1. 背景与目标

Milestone 1–5 已交付单机闭环与 Raft 集群一致性（管理员可访问任意节点管理全局期望状态）。但目前 Web 端仍仅为 bootstrap 页面，缺少“基础功能完整”的管理面板能力：**资源 CRUD 全量可用、关键动作可达、失败可诊断**。

Milestone 6 的目标是交付一个 **基础功能完整（CRUD 完整）** 的 Admin Web 面板：

- 基于 admin token 登录（本地保存），可在同一入口管理 Nodes/Endpoints/Users/Grants；
- 对齐 `docs/desgin/api.md`，补齐 Web 所需的管理 API 缺口，确保四大资源的 CRUD “真实可用”；
- 订阅相关能力在 Web 可直接交付（复制/格式切换/最小校验），并能在错误时给出清晰提示；
- 具备基本质量门槛（Storybook 组件态 + Playwright 关键路径 E2E），避免“可用但不可维护”。

## 2. 范围与非目标

### 2.1 范围（M6 交付）

**基础体验（框架能力）**

- 登录：输入 `admin_token` 并保存到浏览器本地（localStorage），提供退出/清除能力。
- 全局反馈：统一的错误提示（可读的后端 error.code/message）、加载态与空态组件。
- 危险操作确认：删除、重置 token、rotate shortId、禁用授权等操作需要二次确认。

**视图与操作（管理员：CRUD 完整）**

- Nodes：
  - 列表 + 详情；
  - 创建（加入）：提供 join token 生成与 `xp join --token <token>` 命令模板；加入后自动出现在 nodes 列表；
  - 更新：允许更新节点展示相关元数据（`node_name/public_domain/api_base_url`）。
- Endpoints：列表 + 详情；创建/更新/删除；VLESS 端点支持 rotate shortId。
- Users：列表 + 详情；创建/更新/删除；重置订阅 token。
- Grants：列表 + 详情；创建/更新/删除；启用/禁用与配额/周期策略修改；可编辑 `note`；展示 credentials；展示 usage（不可用时显示 N/A）。

**订阅**

- 在 User 详情中提供订阅入口：raw/base64/clash 的一键复制与格式切换。
- 快速校验：对订阅请求做最小校验（HTTP 200 + 非空 + 基本格式），并提供可读错误。

**质量**

- Storybook：覆盖新增组件的典型/异常状态（loading/empty/error/disabled）。
- Playwright：覆盖关键路径 E2E（登录、资源列表可见、创建/删除最小闭环等）。

### 2.2 非目标（明确不做）

- 不引入账号体系、RBAC、多管理员：仍为单 admin token。
- 不在浏览器内生成节点私钥/CSR 并直接调用 `/api/cluster/join`（避免把密钥材料引入浏览器风险面）。
- 不实现节点成员缩容/移除与自动 reconfiguration（涉及 Raft membership 变更与高风险运维流程）。
- 不实现复杂的订阅“客户端连通性探测”（只做最小校验，避免替用户做过多假设）。
- 不要求离线可用与复杂缓存策略（依赖 TanStack Query 默认缓存即可）。
- 不做“规模化效率增强”：搜索/筛选/分页、批量操作、导入/导出（可作为后续工作项单独立项）。

## 3. 关键用例 / 用户流程

1. 登录与导航：
   - 访问 Web → 输入 admin token → 进入 Dashboard → 可切换到 Nodes/Endpoints/Users/Grants 页面。
2. 创建端点并为用户授权（完整闭环）：
   - Endpoints → Create → 选择 node/kind/port… → 创建成功；
   - Users → Create → 创建成功（可后续更新 display_name/周期默认值）；
   - Grants → Create → 选择 user + endpoint → 设置 quota/周期策略/note → 创建成功；
   - Grants → Detail → 复制 credentials / 查看 usage。
3. 订阅交付：
   - Users → Detail → 一键复制订阅链接 → 切换 raw/base64/clash → 展示校验结果与可读错误（如失败）。
4. 风险操作：
   - 删除 endpoint/user/grant；重置 token；rotate shortId；禁用 grant → 均需确认 → 成功后列表/详情刷新。
5. 节点加入（创建 Node）：
   - Web 生成 join token → 复制 `xp join` 命令模板 → 在新节点执行加入 → Web Nodes 列表出现新节点。

## 4. 页面与信息架构（IA）

- `/login`：admin token 输入、保存、清除。
- `/`（Dashboard）：
  - Backend health（`GET /api/health`）
  - Cluster info（`GET /api/cluster/info`）
  - Alerts（`GET /api/admin/alerts`，需要 admin token）
- `/nodes`、`/nodes/$nodeId`：节点列表/详情（含 join token 卡片与可编辑字段）。
- `/endpoints`、`/endpoints/new`、`/endpoints/$endpointId`：端点列表/创建/详情（详情内支持更新/删除/rotate）。
- `/users`、`/users/new`、`/users/$userId`：用户列表/创建/详情（详情内支持更新/删除/reset token + 订阅卡片）。
- `/grants`、`/grants/new`、`/grants/$grantId`：授权列表/创建/详情（详情内支持更新/删除 + usage 卡片）。

> 导航：顶部 navbar + 左侧 tab（或二级导航），保证在小屏可用；默认使用 DaisyUI 组件语义。

## 5. 数据 / 领域模型与前端状态

### 5.1 领域对象（来自 API）

前端以 `docs/desgin/api.md` 为准，核心字段：

- Node：`node_id / node_name / api_base_url / public_domain`
- Endpoint：`endpoint_id / node_id / tag / kind / port / meta`
- User：`user_id / display_name / subscription_token / cycle_policy_default / cycle_day_of_month_default`
- Grant：`grant_id / user_id / endpoint_id / enabled / quota_limit_bytes / cycle_policy / cycle_day_of_month / note / credentials`
- Grant usage：`cycle_start_at / cycle_end_at / used_bytes / ...`

### 5.2 前端状态原则

- admin token：
  - 仅保存在浏览器 localStorage；
  - 通过统一的 “auth state” 在组件树中读取；
  - token 为空时禁止发起 `/api/admin/*` 请求，并引导登录。
- 数据请求：
  - 使用 TanStack Query 管理缓存与重试；
  - 写操作成功后按 queryKey 精准失效（invalidate）相关列表与详情。

## 6. 接口对接与错误处理

### 6.1 M6 必须具备的管理 API（与 `docs/desgin/api.md` 对齐）

为达成“CRUD 完整”，M6 必须具备以下能力（缺一则视为未完成）：

- Nodes：`PATCH /api/admin/nodes/{node_id}`（更新展示元数据）；节点“创建”通过 join 流程完成。
- Endpoints：`PATCH /api/admin/endpoints/{endpoint_id}`（更新端口与协议 meta，约束见 API 文档）。
- Users：`PATCH /api/admin/users/{user_id}`（更新 display_name 与周期默认值）。
- Grants：`PATCH /api/admin/grants/{grant_id}` 支持编辑 `note`（与 enabled/quota/cycle 同一接口）。

> 节点移除/缩容与成员变更属于高风险能力，不纳入 M6（如需支持，应以独立工作项单独设计与实现）。

### 6.2 Web API 模块边界

在 `web/src/api/` 中按资源拆分：

- `adminNodes` / `adminEndpoints` / `adminUsers` / `adminGrants` / `adminAlerts`
- `subscription`（调用 `/api/sub/{token}`，不需要 admin token）

每个模块包含：

- Zod Schema（响应解析）
- `fetch*` / `create*` / `delete*` / `patch*` 等函数

### 6.3 CRUD 映射（页面 → API）

> 目标：所有基础操作都能在 Web 端“真实可用”，且每个动作都有明确对应的 API 调用。

- Nodes
  - list/detail：`GET /api/admin/nodes`、`GET /api/admin/nodes/{node_id}`
  - create（加入流程入口）：`POST /api/admin/cluster/join-tokens`（只生成 join token 并展示 `xp join --token <token>` 命令模板）
  - update：`PATCH /api/admin/nodes/{node_id}`
- Endpoints
  - list/detail：`GET /api/admin/endpoints`、`GET /api/admin/endpoints/{endpoint_id}`
  - create/delete：`POST /api/admin/endpoints`、`DELETE /api/admin/endpoints/{endpoint_id}`
  - update：`PATCH /api/admin/endpoints/{endpoint_id}`
  - rotate shortId：`POST /api/admin/endpoints/{endpoint_id}/rotate-shortid`
- Users
  - list/detail：`GET /api/admin/users`、`GET /api/admin/users/{user_id}`
  - create/update/delete：`POST /api/admin/users`、`PATCH /api/admin/users/{user_id}`、`DELETE /api/admin/users/{user_id}`
  - reset token：`POST /api/admin/users/{user_id}/reset-token`
- Grants
  - list/detail：`GET /api/admin/grants`、`GET /api/admin/grants/{grant_id}`
  - create/update/delete：`POST /api/admin/grants`、`PATCH /api/admin/grants/{grant_id}`、`DELETE /api/admin/grants/{grant_id}`
  - usage：`GET /api/admin/grants/{grant_id}/usage`
  - alerts：`GET /api/admin/alerts`

### 6.4 错误展示规范

- 后端错误形状：`{ error: { code, message, details } }`（见 `docs/desgin/api.md`）。
- UI 展示：
  - 列表页/详情页：页面内可见的错误块（含 `status + code + message`）；
  - 全局：对写操作失败提供 toast/snackbar（避免用户找不到错误）。

### 6.5 follower 写入一致性

后端的 Raft 层可将写请求 forward 到 leader（对前端透明）。前端仅需按常规处理 HTTP 失败场景即可。

## 7. 组件与模块划分（Web）

必须新增通用组件（均需 Storybook 覆盖关键状态）：

- `AppLayout`：navbar + 主内容区域 + 全局消息容器
- `AuthGate`：路由守卫（无 token → 重定向到 `/login`）
- `PageState`：loading/empty/error 统一展示（列表与详情复用）
- `ConfirmDialog`：危险操作确认
- `Toast`：写操作反馈
- `CopyButton`：复制到剪贴板 + 成功/失败反馈
- `ResourceTable`：统一表格样式（排序可后置）

视图层按资源拆分 `web/src/views/*`，每个页面只负责组合 Query + 组件，不在视图里直接写 fetch 细节。

## 8. 测试计划（M6）

### 8.1 Storybook

- 核心组件在以下状态均要有 story：
  - default / loading / empty / error / disabled
- 表单组件补充：
  - 校验失败的展示（例如端口非法、必填缺失等）

### 8.2 Playwright（关键路径）

必须覆盖（最小集）：

1. 登录页可保存 token，并进入 Dashboard；
2. 资源列表页可渲染（nodes/endpoints/users/grants）；
3. 创建 + 删除最小闭环（至少覆盖 users/grants 任意一个资源）；
4. 用户详情页订阅卡片可生成并复制链接（使用页面 API mock 或测试后端）。

## 9. 验收点（DoD 摘要）

- CRUD：在 Web 中可完成 Endpoints/Users/Grants 的创建/查询/更新/删除；Nodes 支持查询与元数据更新，并提供加入指引。
- 关键动作：rotate shortId、reset token、Grant 启用/禁用与配额/周期更新均可用，且失败时提示可诊断。
- 订阅：User 详情提供 raw/base64/clash 切换与复制，并能做最小校验提示成功/失败。
- 质量：Storybook 覆盖核心组件态；Playwright 覆盖关键路径（登录 + 至少一个“创建→更新→删除”闭环）。

## 10. 风险点与待确认问题

1. Nodes 的“删除/移除”与 Raft membership 变更：属于高风险能力，不纳入 M6；如需纳入 MVP，请主人明确拍板并另立工作项。
2. Endpoints/Users 的更新语义：哪些字段可变、哪些不可变（例如 Endpoint.kind/tag 是否固定）需在 API 文档中写死规则。
3. 订阅“最小校验”的边界：必须坚持最小化（HTTP 200 + 非空 + 基础前缀/可读错误），避免把客户端兼容性问题转嫁到服务端。
