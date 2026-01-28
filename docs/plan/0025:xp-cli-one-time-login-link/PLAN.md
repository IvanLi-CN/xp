# xp：CLI 一次性登录链接（Admin UI）（#0025）

## 状态

- Status: 已完成
- Created: 2026-01-26
- Last: 2026-01-27

## 背景 / 问题陈述

- 现状：Admin UI 需要在 `/login` 手工粘贴 `admin_token`（本地存储在浏览器 localStorage），对远程运维、临时登录、多人协作不够顺畅。
- 诉求：提供一个 `xp` 的 CLI 命令，生成**短期有效**的“登录链接”，打开链接即可完成登录（不需要手工复制 token）。
- 安全目标：登录链接不直接泄露 `admin_token`；链接 token 可过期，且在浏览器中会被尽快从地址栏移除，降低泄露面。

## 目标 / 非目标

### Goals

- 为 `xp` 增加一个 CLI 命令：生成 Admin UI 的一次性登录链接（默认短 TTL）。
- Admin UI 支持“打开链接后自动登录”：验证 token 成功后写入 localStorage，并导航到 `/`。
- 服务端的 `/api/admin/*` 鉴权支持一种“派生的短期 token”（JWT，短 TTL；与现有 `admin_token` 并存），用于承载该登录流程。

### Non-goals

- 不引入多管理员/RBAC/细粒度权限模型（仍是单管理员能力面）。
- 不引入持久化 session/cookie 登录体系。
- 不在本计划内实现 strict single-use（用后作废）与跨节点一致防重放机制（本期仅短 TTL 到期作废）。
- 不强制 CLI 自动打开浏览器（可作为后续独立需求）。

## 用户与场景（Users & Scenarios）

- 运维/管理员在服务器或本地环境中，需要快速进入 Admin UI（避免手工复制粘贴 token）。
- 通过 Cloudflare Tunnel 或其他反代方式远程访问 Admin UI，需要可复制的“临时登录入口”。
- 临时协作：为短时排障提供一个可控有效期的登录链接（注意：是否允许转发由安全策略决定）。

## 范围（Scope）

### In scope

- 新增 CLI：`xp login-link`（名称与参数以契约为准；见 `./contracts/cli.md`）。
- Admin UI `/login` 支持从链接携带的 `login_token` 自动登录，并在成功后从 URL 中移除该参数。
- 服务端鉴权扩展：`Authorization: Bearer <token>` 同时接受 `admin_token`（现有）与 `login_token`（新增，短期有效）。
- 文档同步：更新 `docs/desgin/api.md` 中的管理员认证说明（增加 `login_token` 语义与约束）。

### Out of scope

- 向外部系统分发/吊销/审计该登录 token 的管理后台与流程。
- “链接打开即跨设备持久登录”的体验优化（例如记住设备、刷新 token 等）。

## 需求（Requirements）

### MUST

- CLI 能在不启动服务的前提下生成登录链接（依赖本地配置与必要文件即可）。
- 登录链接包含一个短期有效的 `login_token`，且**不等于** `admin_token` 明文。
- `login_token` 采用 JWT（HS256）签名；服务端验证包含：过期校验 + 签名校验 + cluster 绑定校验。
- `login_token` 的 TTL 固定为 `3600` 秒（1 小时）。
- Admin UI 自动消费登录链接参数：
  - 成功：写入 localStorage 并跳转到 `/`；
  - 失败：展示可行动错误信息，不写入 localStorage。
- URL 中的 token 参数在消费后必须被移除（避免长期留在地址栏/历史记录中）。
- 与现有 `admin_token` 登录方式完全兼容（不破坏现有手工登录与 API 客户端）。

### SHOULD

- None

### COULD

- None

## 约束与风险（Constraints & Risks）

- 约束：必须与现有 `admin_token` 登录与 API 客户端完全兼容。
- 约束：`login_token` 通过 URL 传递，存在被浏览器历史、剪贴板、反代 access log 记录的风险；需依靠“短 TTL + 消费后清理 URL + 使用方自律”降低泄露面。
- 风险：若要求 strict single-use 且跨节点一致，会引入新状态与清理逻辑，设计与实现成本显著上升。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                                       | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers）                 | 备注（Notes）                            |
| -------------------------------------------------- | ------------ | ------------- | -------------- | ------------------------ | --------------- | ----------------------------------- | ---------------------------------------- |
| `xp login-link`                                    | CLI          | external      | New            | ./contracts/cli.md       | xp              | operator / automation               | 生成 Admin UI 登录链接                   |
| Admin UI 入口：`GET /login`（含 query 参数）       | HTTP API     | external      | Modify         | ./contracts/http-apis.md | xp/web          | browser                             | 仅定义“参数与消费语义”；服务端仍返回 SPA |
| 管理员鉴权：`Authorization` 规则（`/api/admin/*`） | HTTP API     | external      | Modify         | ./contracts/http-apis.md | xp              | web admin UI / scripts / CLI client | `admin_token` + `login_token` 并存       |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/cli.md](./contracts/cli.md)
- [contracts/http-apis.md](./contracts/http-apis.md)

## 验收标准（Acceptance Criteria）

- Given 已有可用的 `admin_token` 且 `xp` 的 Admin UI 可访问，
  When 执行 `xp login-link`，
  Then 输出一条可复制的 URL，且该 URL 中不包含 `admin_token` 明文。
- Given 在浏览器中打开上述 URL，
  When `login_token` 仍在有效期内且服务端验证通过，
  Then Admin UI 自动登录（localStorage 写入 token）并导航到 `/`，且地址栏中不再包含 `login_token` 参数。
- Given `xp login-link` 生成的 URL，
  When 解析其中的 `login_token`，
  Then JWT `exp` 与生成时刻相比不超过 `3600` 秒。
- Given `login_token` 已过期或被篡改，
  When 在浏览器中打开链接并尝试自动登录，
  Then 页面展示明确错误（区分“过期/无效”至少其一），且不会写入 localStorage。
- Given 使用 `admin_token` 的现有登录方式，
  When 运维人员仍按原流程在 `/login` 粘贴并验证，
  Then 行为保持不变（兼容性不回归）。

## 实现前置条件（Definition of Ready / Preconditions）

- 已冻结 “一次性” 的语义为：短 TTL 到期作废（不做 strict single-use）。
- 已冻结 JWT claims 与签名方案（见 `./contracts/http-apis.md`），并确认其与现有集群/反代部署形态兼容。
- 已冻结 CLI 的命令名、参数与输出口径（见 `./contracts/cli.md`）。
- 已明确 Admin UI 的参数名称与“移除 URL 参数”的实现策略（避免泄露到历史记录/分享链接）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust unit tests: `login_token` 的 encode/decode/validate（过期、签名不匹配、cluster_id 不匹配等）。
- Rust HTTP tests: `login_token` 可访问至少一个 `/api/admin/*` 读接口（例如 `GET /api/admin/alerts`），并返回 401 于无效 token。
- Web unit tests (Vitest): `/login` 能从 URL 参数读取 token、触发验证、成功后写入 localStorage，并移除 URL 参数。

### Quality checks

- Backend: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`
- Frontend: `cd web && bun run lint`, `cd web && bun run typecheck`, `cd web && bun run test`

## 文档更新（Docs to Update）

- `docs/desgin/api.md`: 管理员认证补充 `login_token`（语义、约束、与 `admin_token` 的并存规则）。
- `docs/desgin/cluster.md`: 如涉及对外访问/反代，补充“登录链接”的安全注意事项与推荐用法（若需要）。
- `docs/ops/README.md`: 记录运维如何生成与使用登录链接（若需要）。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [x] M1: 后端鉴权支持 `login_token` JWT（HS256；`cluster_id` 绑定；`exp` 校验）
- [x] M2: 新增 `xp login-link`（输出 URL；TTL 固定 3600s）
- [x] M3: Admin UI `/login` 自动消费 `login_token`（验证、落盘、清理 URL）+ 测试与文档更新

## 方案概述（Approach, high-level）

- 复用 Join token 的签名思路：`login_token` 采用 base64url(JSON payload) + HMAC 签名，服务端用 `admin_token` 作为验证密钥。
- Admin UI 打开链接后，在 `/login` 页面读取 query 中的 token，先做一次可验证请求（如 `GET /api/admin/alerts`），成功后写入 localStorage 并清理 URL。
- 服务端保持向后兼容：继续接受 `admin_token`，并新增对 `login_token` 的验证分支。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 假设：
  - 允许登录链接在对公网可访问的域名上使用（依赖短 TTL 与运维侧谨慎转发）。
  - 本计划的 “一次性” 解释为：短 TTL 到期作废（不做 strict single-use）。

## 变更记录（Change log）

- 2026-01-26: 创建计划。
- 2026-01-27: 实现 `xp login-link` + JWT `login_token` 鉴权 + `/login` 自动消费登录链接。

## 参考（References）

- `docs/desgin/api.md`: 管理员认证（`Authorization: Bearer <admin_token>`）
- `docs/plan/0008:m6-web-panel/PLAN.md`: `/login` 页面与登录流程背景
- `src/config.rs`: `xp` CLI 定义（clap）
- `src/http/mod.rs`: `admin_auth` 中的 Bearer token 鉴权
- `src/cluster_identity.rs`: Join token 的 HMAC 签名与校验方式（可复用设计模式）
- `web/src/views/LoginPage.tsx`: 登录页逻辑（需要支持自动消费登录链接）
