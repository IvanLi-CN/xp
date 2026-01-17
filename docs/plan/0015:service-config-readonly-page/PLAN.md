# 服务配置只读展示页（#0015）

## 状态

- Status: 待实现
- Created: 2026-01-17
- Last: 2026-01-17

## 背景 / 问题陈述

- 目前 Web 管理端缺少“服务配置”可视化入口，运维只能通过 CLI/ENV/文件判断生效配置。
- 排障与核对配置（监听地址、数据目录、配额轮询等）需要一次性可视化确认。
- 新增只读页可降低误配风险与沟通成本，同时不引入配置变更能力。
- 现有字段名 `public_domain` 与“订阅/客户端连接 host（允许 IP）”语义不贴合，需更贴切命名。

## 目标 / 非目标

### Goals

- 在 Web 管理端新增只读“服务配置”页面，集中展示当前进程的关键配置值。
- 提供一个安全的 Admin API，返回**可公开给管理员**的配置字段（避免泄露敏感信息）。
- 页面支持手动刷新，错误状态可读。
- 将“订阅/客户端 host”字段重命名为更贴切的名称：`access_host`（全量重命名）。

### Non-goals

- 不提供任何配置修改/写入能力。
- 不展示敏感密钥明文（例如 `admin_token`）。
- 不覆盖集群/节点的业务配置详情（如 Endpoint/Grant 详情），仅展示服务进程级配置。
- 不引入新的配置来源或运行时开关。

## 用户与场景

- 用户：控制面管理员/运维。
- 场景：部署/升级后核对服务监听地址、数据目录、配额轮询参数是否符合预期；排障时快速确认配置生效值。

## 范围（Scope）

### In scope

- 新增 UI 页面与导航入口：只读展示配置字段（入口在 Settings 分组）。
- 新增 Admin HTTP API：读取并返回安全的配置视图（只读）。
- 前端数据模型与错误处理（缺失 token/请求失败/空值展示）。
- `public_domain` → `access_host` 全量重命名（配置、域模型、API、持久化与文档）。

### Out of scope

- 配置编辑、导出、版本对比。
- 展示敏感材料（私钥、证书、完整 token）。
- 引入新的配置来源或运行时热更新能力。

## 需求（Requirements）

### MUST

- 新增 `GET /api/admin/config`（或等价路径）返回只读配置视图。
- 返回字段至少包含：`bind`、`xray_api_addr`、`data_dir`、`node_name`、`access_host`、`api_base_url`、`quota_poll_interval_secs`、`quota_auto_unban`。
- `admin_token` 仅以“存在性 + 脱敏展示”形式返回（不得返回明文）；脱敏值长度与实际 token 长度一致，全部打码（`*`）。
- UI 页面在无有效管理员 token 时提示需要登录/授权。
- UI 页面支持刷新，并展示加载/错误/空态。
- `public_domain` 字段在配置/域模型/API/持久化/文档中全部替换为 `access_host`。

### SHOULD

- 配置字段按“网络/节点/配额/安全”分组展示。
- 提供可复制的非敏感字段（如 `api_base_url`、`bind`）。

### COULD

- 显示配置来源（flag/env/default），如后端已有可用信号（当前无则不实现）。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）       | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）     | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）           |
| ------------------ | ------------ | ------------- | -------------- | ---------------------------- | --------------- | ------------------- | ----------------------- |
| AdminServiceConfig | HTTP API     | internal      | New            | ./contracts/http-apis.md     | backend         | web                 | 新增只读配置读取接口    |
| AdminNodes         | HTTP API     | internal      | Modify         | ./contracts/http-apis.md     | backend         | web                 | Node 字段改名           |
| ServiceConfigPage  | UI Component | internal      | New            | ./contracts/ui-components.md | web             | app                 | 只读展示视图模型        |
| XpCliConfig        | CLI          | internal      | Modify         | ./contracts/cli.md           | backend         | ops                 | 配置参数改名            |
| PersistedState     | File format  | internal      | Modify         | ./contracts/file-formats.md  | backend         | ops                 | state/metadata 结构改名 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/cli.md](./contracts/cli.md)
- [contracts/file-formats.md](./contracts/file-formats.md)
- [contracts/ui-components.md](./contracts/ui-components.md)

## 约束与风险

- 约束：不得泄露 `admin_token` 等敏感信息；字段需明确“安全可展示”边界。
- 约束：保持与现有管理端鉴权/错误格式一致。
- 风险：字段与真实生效值不一致（例如配置在启动时被覆盖），需要明确“以当前进程 Config 为准”。
- 风险：字段重命名为破坏性变更，旧客户端/脚本将失效，需明确迁移方案。

## 验收标准（Acceptance Criteria）

- Given 管理员已登录并持有有效 token
  When 打开“服务配置”页面
  Then 页面展示配置字段与分组标题，数值与后端返回一致。
- Given `admin_token` 已配置
  When 页面展示安全字段
  Then `admin_token` 仅显示“存在/脱敏形式”，不出现明文，且脱敏长度与实际一致。
- Given API 返回 401 或网络错误
  When 页面加载
  Then 显示错误态与可重试操作。
- Given 配置字段为空或缺失
  When 页面展示
  Then 使用明确占位（如 “(empty)”）并不崩溃。
- Given 已完成字段重命名
  When 调用任意 Node 相关 Admin API
  Then 返回/请求字段为 `access_host`，不存在 `public_domain`。
- Given 存在旧版持久化数据（含 `public_domain`）
  When 服务启动并加载数据
  Then 旧字段被迁移为 `access_host`，并写回新格式。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `src/http/tests.rs` 增加 `GET /api/admin/config` 的成功/未授权/脱敏字段测试。
- Unit tests: `src/http/tests.rs` 增加 Node API 的字段改名覆盖（list/get/patch）。
- Unit tests: `src/state.rs` 增加旧字段迁移与 schema_version 校验测试。
- Integration tests: 无新增（如后端已有 admin 测试基建，复用其 helper）。
- E2E tests (if applicable): 可选，若已有 Playwright 流程则补充只读页面可加载即可。

### UI / Storybook (if applicable)

- Stories to add/update: 为 ServiceConfigPage 添加基础展示与错误态 story。
- Visual regression baseline changes (if any): 如新增 story 需更新基线。

### Quality checks

- Backend: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`。
- Web: `bun run lint`, `bun run typecheck`, `bun run test`（如新增测试）。

## 文档更新（Docs to Update）

- `docs/plan/README.md`: 新增本计划索引行。
- `docs/desgin/api.md`: Node 字段名统一为 `access_host`。
- `docs/desgin/subscription.md`: 订阅 host 字段描述更新。
- `docs/desgin/architecture.md`: Node 元数据字段更新。
- `docs/desgin/workflows.md`: 初始化与订阅步骤字段名更新。

## 里程碑（Milestones）

- [ ] M1: 冻结接口契约（HTTP API + UI 视图模型）。
- [ ] M2: UI 与 API 的实现与自测完成。
- [ ] M3: 测试/Storybook/文档更新完成并满足质量门槛。

## 方案概述（Approach, high-level）

- 后端新增只读 API，返回 Config 的“安全子集”；明确字段集合与脱敏策略。
- 前端新增路由与页面组件，按分组展示并复用现有 PageState/Button 等组件。
- 通过统一错误格式与 admin 鉴权中间件保持一致性。
- 字段重命名通过一次性迁移完成，启动时自动处理旧数据格式并写回新格式。

## 风险与开放问题（Risks & Open Questions）

- 风险：字段口径与运维期望不一致（例如 data_dir 的相对路径展示）。
- 风险：全量重命名导致旧工具链不可用，需同步升级。
- 需要决策的问题：见“开放问题”。

## 开放问题（需要主人回答）

- None

## 假设（已确认）

- 默认新增路径为 `/service-config`，并在导航中新增“Service config”。
- 脱敏策略为“按实际长度全量打码”，空值显示空字符串。
- `access_host` 为最终字段名，旧字段不保留。
- 启动时自动迁移旧数据（`public_domain` → `access_host`），并写回新格式（schema_version 递增）。

## 参考（References）

- `src/config.rs`
- `web/src/api/backendError.ts`
- `web/src/router.tsx`
