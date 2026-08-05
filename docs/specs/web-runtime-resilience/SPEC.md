# Web 运行时韧性

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- 已打开的 PWA 标签页可能在服务端升级后继续运行旧 Web bundle；框架级异常目前退化为未设计的通用错误输出。
- 现有 PWA 已具备 app shell 预缓存和离线读缓存，但需要验证构建资产完整性与版本切换原子性。
- Web 与服务端 API 的兼容边界尚未形成可测试的发布合同。

## 目标 / 非目标

### Goals

- 提供符合管理台设计的框架级错误恢复界面，并保留可复制的脱敏诊断证据。
- 确保单个 PWA 版本的完整静态资产被主动缓存，并仅在新版本完整可用后切换。
- 让 Web 与当前及前两个 minor 服务端版本保持双向兼容，缺失能力按功能局部降级。

### Non-goals

- 不自动刷新正在使用的页面或中断用户操作。
- 不清除登录状态、IndexedDB 查询缓存或全部站点数据。
- 不缓存通用认证 API 响应，不增加离线写入队列。
- 不保证超过前两个 minor 的服务端兼容。

## 范围（Scope）

### In scope

- 根级 React / Router 错误边界、错误分类、诊断脱敏与恢复操作。
- Workbox precache 完整性、waiting update、原子激活和旧缓存清理时机。
- N/N-1/N-2 API fixtures、宽容 schema、能力探测与局部降级。
- Storybook、`ui_demo`、Vitest、Playwright、后端合同测试和相关文档。

### Out of scope

- Service Worker 中的认证 API runtime cache。
- 自动后台切换新 bundle。
- 全局服务端版本阻断页。

## 需求（Requirements）

### MUST

- 框架级异常必须落入稳定错误界面，而不是框架默认错误输出或空白页。
- 错误界面必须区分资源加载、缓存版本不一致、离线、API 不兼容、React runtime 和未知错误。
- 默认恢复操作必须为受控刷新；仅在资源/缓存不一致或重复失败时提供静态应用缓存清理。
- 静态缓存清理不得删除 token、UI 偏好或 IndexedDB 查询缓存。
- 每次 Web 构建的入口 HTML、JS chunks、CSS、字体、图标和 manifest 必须属于同一 precache 版本。
- 新 Service Worker 必须在完整安装后进入 waiting；incumbent 仍控制客户端时仅在用户确认后请求激活。
  零受控客户端时允许浏览器按原生生命周期自动激活，下一次访问直接使用完整新构建。
  当前可用缓存不得因失败安装而被清理，仍有旧构建客户端时不得提前删除其完整 precache。
- Web 必须兼容当前及前两个 minor 服务端版本，覆盖新 Web 连接旧后端与旧 Web 连接新后端。
- 缺失 API 或字段必须在相关功能局部降级，不得阻断整个 App。

### SHOULD

- 技术诊断默认折叠并支持复制，内容包含 Web build ID、错误类别、当前路径和脱敏堆栈。
- 更新提示应保持独立于后端升级状态，并避免打断表单或运维操作。

### COULD

- 对重复恢复失败记录同标签页内的短期故障标记，用于显示更强的恢复建议。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 框架异常发生后，错误边界分类异常并呈现对应说明、建议和允许的恢复操作。
- `Reload app` 保留站点数据并重新导航；条件式缓存恢复先在线下载并校验完整 replacement precache，
  再导航到 replacement build。旧页面及其 ownership 保留到导航提交、旧 client 从
  `clients.matchAll()` 消失；此后才原子替换不再被其他客户端持有的 XP app-shell cache。
  无新版 worker 且无法完整重建当前 build 时不得删除可用缓存，只提供普通 reload 与稍后重试。
- 新构建由 Service Worker 后台完整获取；正常状态展示可关闭的更新提示，用户确认后切换。
- API consumer 以宽容 schema 和能力存在性决定功能可用性；旧服务端缺失的新功能显示局部升级说明。

### Edge cases / errors

- React 挂载前失败或错误界面自身再次失败时，document 级无依赖静态 fallback 必须接管；
  该路径不得依赖 React、Router、动态 chunk 或 Service Worker 消息往返。
- 新 Service Worker 安装失败或网络中断时，当前激活版本及其缓存继续工作。
- 多标签页可以暂时运行不同完整构建，但单个标签页不得混用不同构建的静态资源；
  旧版本 precache 必须保留到对应受控客户端全部离开，再由后续激活或启动清理。
- 超出兼容窗口或缺少关键页面合同的服务端返回明确的局部不兼容状态。
- cache recovery 若发现目标 build 仍被其他客户端持有，必须保留该 precache；
  请求页面仍可更新并导航到 active build，界面说明旧缓存将在最后一个 owner 离开后清理。

## 接口契约（Interfaces & Contracts）

- 不把 PWA build ID 与服务端 API 版本绑定。
- API 兼容版本只取仓库 `vMAJOR.MINOR.PATCH` 正式 release tag；排除 prerelease。
  `N` 是 Spec@baseline 可达的最高正式 release minor，本 Initiative 固定为 `3.22`，
  因而窗口为 `3.22`、`3.21`、`3.20`。
- 后端 API 在兼容窗口内保持旧 Web 已使用字段与语义；新增字段默认 additive。
- Web schema 对兼容窗口内缺失的可选字段提供明确默认值，未知新增字段保持可忽略。
- 构建测试必须分别从 `N` 的 `v3.22.5`，以及 `N-1`、`N-2` 各 minor 最新正式 release tag
  提取不可变的 Web `/api`、`/events` callsites、parser identity、完整 API source digest 与后端路由
  inventory；双向合同必须覆盖三份 inventory 的 method、path、status 与 capability 并集，禁止以
  当前源码或抽样页面替代。真实 wire-body fixture 与宽容 schema 解析覆盖关键页面，包括 nodes、
  config、status events、version check 与新增/缺失字段场景；不以合成空 body 冒充未建模响应合同。
- 每个受控页面必须向 Service Worker 声明自己的 Web build ID；Service Worker 维护
  `clientId -> build ID` ownership，并将该客户端的静态请求路由到对应保留 precache。
  ownership 写入独立的 `xp_sw_metadata` 持久存储，不得复用或清除 `react_query_cache`。
  新 worker 激活或 controller change 后必须用 `clients.matchAll()` 与页面消息重新核对持久记录；
  reconciliation 完成前禁止清理任何旧 build cache，已消失 client 的记录才可回收。
  已存在客户端按 ownership 取完整旧构建；新的或重新加载的导航始终取完整 active build 的
  precached `index.html`，随后声明该 active build。页面声明到达前，子资源仅可从此次导航选择的
  build precache 返回；不明确的请求必须失败并进入恢复界面，不得从另一构建猜测响应。
- 当前后端新增 additive `/api/capabilities`，返回稳定 capability IDs；当前 Web 优先使用它。
  在 N-1/N-2 后端该接口缺失时，Web 使用 `/api/version/check` 的 `current.release_tag`；
  仅接受 `vMAJOR.MINOR.PATCH` 正式 tag 并按 major/minor 选择仓库内固定的 release capability profile。
  若 version check 因外部 GitHub 查询失败而不可用，则执行从三份 immutable release inventory
  生成的只读、本地 endpoint/schema fingerprint probes。唯一匹配时选择对应 profile；
  多个 minor 只有在当前 consumer 所需 capability 集完全相同时才组成等价类并使用该共同 profile。
  无匹配、非等价多匹配、无效、prerelease 或窗口外 tag 进入明确的不兼容状态。
  只有 profile 明确声明缺失的 endpoint/field 才能局部降级；
  已声明能力出现 404 或 schema failure 必须作为回归错误，不得伪装成兼容降级。

## 验收标准（Acceptance Criteria）

- Given chunk 加载失败、React #185 或未知运行时异常，When 根错误边界捕获异常，Then 显示分类后的项目错误界面和可执行恢复动作。
- Given 用户选择清理静态应用缓存且目标 build 无其他 owner，When 恢复完成，Then 对应 XP app-shell
  cache 在完整 replacement 就绪后被原子替换，登录和 IndexedDB 数据仍存在。
- Given 无新版 worker 且网络无法完整重建当前 build，When 用户请求 cache recovery，Then
  当前完整缓存不变，页面说明无法安全清理并提供普通 reload。
- Given 另一标签页仍持有请求清理的 build，When 当前标签页执行 cache recovery，Then
  旧 precache 保留供该 owner 使用，当前标签页切到 active build，其他站点数据不变。
- Given 新构建下载中断，When 页面重新访问，Then 当前完整旧构建仍能启动。
- Given 新构建完整进入 waiting，When 用户确认更新，Then 当前页面切换到单一新构建；
  仍服务旧构建标签页的完整缓存继续保留，最后一个旧客户端离开后才可清理。
- Given 旧标签页尚未加载某个 lazy chunk，When 新版本在另一标签页激活，Then 旧标签页仍从自己的完整
  precache 加载该 chunk，不会回退到新构建资产。
- Given N、N-1 或 N-2 服务端合同，When 全量 Web API callsite inventory 与关键页面 wire-body
  fixtures 执行，Then method/path/status/capability 全量匹配，关键页面支持的功能正常工作，缺失能力
  仅在对应 consumer 局部降级。
- Given N-1 或 N-2 Web 的版本化请求/响应合同，When 合同针对当前后端执行，Then 既有 endpoint、请求格式、
  状态码和响应语义继续满足旧客户端预期。
- Given React bootstrap 或错误界面自身抛错，When document 级兜底接管，Then 页面仍提供最小错误说明与重新加载操作。

## 验收清单（Acceptance checklist）

- [x] 核心路径的长期行为已被明确描述。
- [x] 关键边界/错误场景已被覆盖。
- [x] PWA 与 API 兼容边界保持独立。
- [x] 相关验收条件可以用于实现与 review 对齐。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 错误分类、诊断脱敏、缓存选择、schema 默认值与能力降级。
- Integration tests: embedded assets、Service Worker cache headers、当前 Web 对 N/N-1/N-2
  method/path/status/capability inventory 与关键页面 wire-body fixtures，以及 N-1/N-2 Web
  版本化请求合同对当前后端。
- E2E tests: 离线 warm-load、waiting update、失败安装、旧标签页 lazy chunk、用户触发切换，
  以及 React bootstrap/fallback 二次失败。

### UI / Storybook

- 根错误页的分类状态必须有稳定 Storybook stories 与行为关键 `play` 覆盖。
- 整页视觉证据来自稳定 `ui_demo`，覆盖桌面和移动视口。

### Quality checks

- `cargo test`
- `bun run check:style-budget`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run build-storybook`
- `cd web && bun run test-storybook`
- `cd web && bun run test:e2e:pwa`

## Visual Evidence

- Source: stable mock-only `ui_demo` route `/ui-demo/framework-recovery`.
- Coverage: desktop and mobile viewports, including first-failure and repeated-failure states.
- PR: none; no screenshot assets are committed or referenced.

## 文档更新（Docs to Update）

- `docs/specs/7qj3h-web-pwa-offline-console/`
- `docs/solutions/web/pwa-offline-admin-shell.md`
- `docs/ops/**`
- `AGENTS.md`
- 适用的项目入口文档

## 实现前置条件（Definition of Ready / Preconditions）

- [x] 主人已批准 Initiative 范围、双 Ticket 边界、API 兼容窗口与最终 owner approval gate。
- [x] 两个 Ticket 已在 canonical GitHub tracker 创建：
      [#226 framework error recovery](https://github.com/IvanLi-CN/xp/issues/226) 与
      [#227 atomic PWA updates and API compatibility](https://github.com/IvanLi-CN/xp/issues/227)。
- [x] PWA 缓存生命周期与 API 兼容合同保持独立，不以 build ID 或缓存键耦合。
- [x] UI 交付需先补 Storybook 状态，再由稳定 `ui_demo` 提供视觉证据。
- [x] 两个 Ticket URL 已回填，dispatch readiness 成立；产品与架构决策已冻结。

## 实现里程碑（Milestones）

- [x] 框架错误分类、项目风格恢复界面、静态缓存恢复动作及 document 级最小 fallback
      可独立验收。
- [x] PWA 完整 precache、waiting 更新、跨标签页旧缓存保留与延迟清理可独立验收。
- [x] N/N-1/N-2 新 Web 对旧后端和旧 Web 对新后端的版本化合同、能力探测与局部降级
      可独立验收。

## 风险 / 假设 / 开放问题

- API fixture 以各 minor 线最后一个正式 Release 的实际响应合同为准。
- 两个 Ticket 可独立实现和回滚；PWA 缓存不得依赖 API 兼容信号。
