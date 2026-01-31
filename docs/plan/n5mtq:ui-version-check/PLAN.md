# Web UI：版本显示与更新检查（#n5mtq）

## 状态

- Status: 部分完成（4/4）
- Created: 2026-01-31
- Last: 2026-01-31

## 背景 / 问题陈述

- Web UI 目前缺少一个“集中、明显”的版本信息入口：运维人员不容易快速确认当前 `xp` 版本。
- UI 现状已依赖后端返回的 `xp_version`（`GET /api/cluster/info`），并且节点部署引导命令会用该版本拼接 GitHub Releases 下载链接（`web/src/views/NodesPage.tsx`）。但这些信息对“我现在跑的是什么版本”并不直观。
- 过往项目经验（对齐口径）：
  - `pod-upgrade-trigger`：后端提供 `GET /api/version/check`，由后端访问 GitHub Releases `releases/latest` 并比较 semver；前端采用“页面聚焦触发 + 冷却时间（例如 1h）”的节流策略，顶栏展示当前版本与“新版本可用”入口。
  - `tavily-hikari`：前端通过“后端版本变化”提示用户刷新页面（不依赖 GitHub），并允许本地 dismiss。
  - `dockrev`：涉及自我升级时用独立执行者（supervisor）与独立页面/接口，避免主服务重启窗口内失联。

## 目标 / 非目标

### Goals

- 在 Web UI 中显示“当前运行的 `xp` 版本”（明确数据来源与含义）。
- 在 Web UI 中提供“新版本检查（update check）”能力：能判断是否存在“更高版本的 release”，并展示可行动信息（例如最新版本号与 release 链接）。
- 失败可诊断：检查失败时不阻塞页面，给出可重试入口与可读错误信息（不泄露敏感信息）。

### Non-goals

- 不在 Web UI 内直接执行升级/替换二进制（升级仍由 `xp-ops` 或运维流程完成）。
- 不做“按节点逐个检测版本漂移 / 混跑版本”的审计与告警（本计划只解决“当前连接到的 control plane 版本 + 是否存在更新”）。
- 不引入新的发布渠道体系（是否包含 prerelease 由本计划决策冻结；本计划默认不含“更多渠道/镜像源”扩展）。

## 范围（Scope）

### In scope

- UI 侧：
  - 顶栏（App header）展示当前版本号（复用 `clusterInfo.xp_version`）。
  - “检查更新”采用与 `pod-upgrade-trigger` 一致的 UX：聚焦触发 + 冷却时间节流；在顶栏提供“新版本可用”的入口（点击跳转 GitHub Release）。
- 后端侧（对齐 `pod-upgrade-trigger` 的版本检查分层）：
  - 新增 `GET /api/version/check`：后端负责访问 GitHub Releases `releases/latest`，并执行 semver 比较（无法比较时 `has_update=null`），返回给前端。
  - 后端必须实现缓存/节流，避免频繁访问 GitHub API；前端同样做聚焦节流，双重防抖。

### Out of scope

- 为 UI 新增“升级按钮/进度条/回滚按钮”等自动化运维能力。
- 在 UI 内配置或管理 GitHub token（本计划默认只做匿名查询；如需 token 或私有仓库，另起计划）。

## 需求（Requirements）

### MUST

- UI 必须显示当前 `xp` 版本：
  - 数据来源：`GET /api/cluster/info` 的 `xp_version`（或与其等价且已在 UI 中复用的来源）。
  - 展示形态：顶栏始终可见（对齐 `pod-upgrade-trigger` 顶栏版本展示习惯）。
  - 交互：版本号必须可点击：
    - 默认行为：点击打开该版本对应的 GitHub Release 页面（新标签页）。
    - 版本 tag 映射：`tag = xp_version` 若已带 `v` 前缀，否则 `tag = "v" + xp_version`（与现有节点部署引导命令保持一致）。
    - 若当前版本无法映射为稳定 tag（例如非 semver 或为空）：降级为打开仓库 Releases 列表页（或显示“无法定位 release”的提示；实现阶段二选一定案）。
- UI 必须提供“更新检查”能力：
  - 触发策略：采用“页面聚焦触发 + 冷却时间节流”（默认 1 小时；可配置/可在实现阶段固化为常量）。
  - 必须有 4 种可区分状态：`checking` / `up_to_date` / `update_available` / `check_failed`（`uncomparable` 作为 `check_failed` 的子原因或以 `has_update=null` 表达）。
  - `update_available` 时必须提供“最新版本号 + release 链接”。
  - `check_failed` 时必须提供可重试入口，并展示简短错误摘要（不包含 secrets）。
- 新版本检查的“版本真相源”必须是 GitHub Release tag（对齐 `pod-upgrade-trigger` 的口径），并保证当前版本的展示来源不会与 release tag 长期漂移（见“实现前置条件”）。
- 后端与前端都必须具备缓存/节流策略，避免频繁请求外部 API（实现方式见契约与方案概述）。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）            | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                                               |
| ----------------------- | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ----------------------------------------------------------- |
| `GET /api/version/check` | HTTP API     | internal      | New            | ./contracts/http-apis.md | backend         | web                 | 对齐 `pod-upgrade-trigger`：返回 current/latest/has_update |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)

## 验收标准（Acceptance Criteria）

- Given Web UI 能成功请求 `GET /api/cluster/info`，
  When 打开 Dashboard（或任一已加载 AppShell 的页面），
  Then UI 显示当前 `xp` 版本（与 `clusterInfo.xp_version` 一致）。
- Given 顶栏已显示当前 `xp` 版本号，
  When 用户点击该版本号，
  Then 浏览器打开对应的 GitHub Release 页面：
  - 若 `xp_version="X.Y.Z"` 则打开 `.../releases/tag/vX.Y.Z`
  - 若 `xp_version="vX.Y.Z"` 则打开 `.../releases/tag/vX.Y.Z`
- Given 当前 `xp` 版本为 `vX.Y.Z`，且 GitHub Releases 存在更新的 stable 版本 `vA.B.C`（`A.B.C > X.Y.Z`），
  When 页面聚焦触发一次版本检查（且距离上次检查超过冷却时间），
  Then UI 显示 `update_available`，并展示 `vA.B.C` 与可点击的 release 链接。
- Given 当前 `xp` 版本为 `vX.Y.Z`，且 GitHub Releases 的最新 stable 为 `vX.Y.Z`，
  When 页面聚焦触发一次版本检查（且距离上次检查超过冷却时间），
  Then UI 显示 `up_to_date`。
- Given 外部网络不可用或 GitHub API 返回错误（如 403 rate limit / 5xx），
  When 页面聚焦触发版本检查或用户手动重试，
  Then UI 显示 `check_failed`，错误信息可读且不泄露敏感数据，并可重试。
- Given 冷却时间与缓存策略已启用，
  When 在冷却窗口内反复聚焦页面，
  Then 不会重复触发版本检查请求（前端节流），且后端不会对 GitHub API 产生同等次数的上游请求（后端缓存）。

## 实现前置条件（Definition of Ready / Preconditions）

- 已确认 `xp` 的“当前版本”来源与 release tag 对齐策略：
  - 当前版本展示应以 `XP_BUILD_VERSION`（`src/version.rs`）为准；当未注入时回退 `CARGO_PKG_VERSION`。
  - UI 展示的 `xp_version` 与 GitHub release tag 的 `vX.Y.Z` 的映射规则已冻结（是否总是 `v${xp_version}`）。
- 已确认“当前版本可点击”的降级策略：当 `xp_version` 无法映射到 release tag 时，是跳转到 releases 列表页，还是显示提示并不跳转。
- 已确认版本检查的节流策略：冷却时间默认值（建议 1 小时，对齐 `pod-upgrade-trigger`）。
- 已确认 `GET /api/version/check` 的错误语义（5xx vs 200+error 字段），以便前后端一致实现与测试。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust:
  - 为 `GET /api/version/check` 增加路由级测试（`src/http/tests.rs`），覆盖：up-to-date / update-available / upstream-failure / caching。
- Web:
  - 为 version-check 的 schema/parse 与状态转换增加单测（`web/src/api/*.test.ts`）。
  - 为“聚焦触发 + 冷却时间（localStorage）”的 hook 增加单测（参照 `pod-upgrade-trigger` 的节流策略）。
  - 为顶栏版本与“新版本可用入口”的展示逻辑增加基础用例（`web/src/views/*.test.tsx` 或 component test）。

### UI / Storybook (if applicable)

- 在现有 Storybook 页面（如 `web/src/views/DesignPages.stories.tsx`）增加一个包含“版本 + 更新状态”展示的场景，覆盖至少 3 种状态（up-to-date / update-available / failed）。

### Quality checks

- Backend: `cargo test`, `cargo fmt`, `cargo clippy -- -D warnings`
- Web: `cd web && bun run lint`, `cd web && bun run typecheck`, `cd web && bun run test`

## 文档更新（Docs to Update）

- `docs/ops/README.md`: 增加一段简短说明“Web UI 会显示当前版本并可检查更新；升级仍建议通过 `xp-ops upgrade` 完成”，并说明更新检查默认来源与可配置项（如复用 `XP_OPS_GITHUB_REPO`）。

## 计划资产（Plan assets）

- None

## 资产晋升（Asset promotion）

- None

## 实现里程碑（Milestones）

- [x] M1: Backend：新增 `GET /api/version/check`（对齐 `pod-upgrade-trigger` 形状；含 GitHub releases/latest 查询 + 缓存/节流 + 测试）
- [x] M2: Web：顶栏“当前版本可点击（release link）”+ 聚焦触发/冷却时间节流的更新提示 + 失败重试（含测试）
- [x] M3: 质量门槛补齐：Storybook 场景覆盖（up-to-date / update-available / failed）+ Vitest 覆盖核心状态机
- [x] M4: 文档：补齐 `docs/ops/README.md` 的版本/更新说明（明确升级仍通过 `xp-ops upgrade`）

## 方案概述（Approach, high-level）

- 后端对齐 `pod-upgrade-trigger`：提供 `GET /api/version/check`，内部复用“从 GitHub Releases 解析 tag + semver 比较 + 缓存”的实现风格（可参考 `xp-ops` 的 `src/ops/upgrade.rs` 但保持 API 形状与职责更贴近版本检查）。
- 前端对齐 `pod-upgrade-trigger`：聚焦触发 + 冷却时间（localStorage）节流；在顶栏展示当前版本与“新版本可用 vX.Y.Z”入口，点击跳转到 release 页面。
- 补充对齐 `tavily-hikari`（可选、默认不做）：若后续引入 SSE/事件，可用“后端版本变化提示刷新页面”的机制作为第二层体验优化（不依赖 GitHub）。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - GitHub API rate limit / 网络不可达导致频繁失败：需要缓存/节流与良好的失败 UI。
  - 版本字符串非严格 semver（例如带 git hash / build metadata）导致比较歧义：需要明确比较规则与降级策略。
- 开放问题（需要决策的问题）：
  - None
- 假设（需主人确认）：
  - 仅检查 stable（GitHub `releases/latest`），不包含 prerelease。
  - `GET /api/version/check` 在上游错误时返回 5xx（对齐 `pod-upgrade-trigger`），前端展示 `check_failed` 并提供重试。
  - 更新来源配置复用 `xp-ops` 的 env override：`XP_OPS_GITHUB_REPO` / `XP_OPS_GITHUB_API_BASE_URL`（默认 `IvanLi-CN/xp` / `https://api.github.com`）。
  - 前端聚焦触发冷却时间默认 1 小时。
  - 当前版本点击跳转：当 `xp_version` 无法映射为 semver tag 时，降级跳转到仓库 Releases 列表页。

## 变更记录（Change log）

- 2026-01-31: 对齐过往项目经验（`pod-upgrade-trigger`）：将版本检查接口与 UX 调整为 `GET /api/version/check` + 聚焦节流
- 2026-01-31: 进入实现阶段：冻结默认行为（stable-only、5xx 错误语义、1h 冷却时间、复用 `xp-ops` env）
- 2026-01-31: 完成实现与验证：M1–M4（backend+web+tests+docs）

## 参考（References）

- `web/src/api/clusterInfo.ts`（`xp_version` 来源）
- `web/src/views/NodesPage.tsx`（部署引导命令中使用 `xp_version` 选择 release tag）
- `docs/ops/README.md`（升级建议与 repo 覆盖配置）
- `src/ops/upgrade.rs`（从 GitHub Releases 解析最新版本的实现风格参考）
