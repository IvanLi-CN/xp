# Web 主后端切换

> 当前有效规范以本文为准；实现覆盖与主题生命周期见
> `./IMPLEMENTATION.md` 和 `./HISTORY.md`。

## Related ADRs

- [ADR-0011](../../adr/0011-cross-origin-primary-backend-for-embedded-pwa.md)

## 背景 / 问题陈述

已安装的嵌入式 PWA 依赖安装来源节点的同源 API。该节点不可访问时，用户必须重新找到其他节点并重新打开
Web 界面，已有缓存和页面上下文无法继续使用。

## 目标 / 非目标

### Goals

- 在 AppShell 右上角提供同集群节点的手动主后端切换。
- 让浏览器 API 和 SSE 请求统一发送到一个已验证的主后端，同时保持 PWA 静态资源 origin
  不变。
- 以当前注册节点的精确 HTTPS origin 作为动态 CORS allowlist，并允许 Authorization
  预检。
- 按 `cluster_id` 持久化已验证后端档案，在主后端不可达时保留缓存和切换入口。
- 对未完成 mutation 实施切换屏障；超时标记未知且不自动重放。

### Non-goals

- 不建设独立静态部署、跨域 cookie、多集群档案或任意 URL 输入。
- 不自动故障转移、后台轮询所有节点或自动重放请求。
- 不改变服务端节点转发、Raft 协调、权限模型或既有全页节点导航。

## 范围（Scope）

### In scope

- Rust `/api` 动态 CORS middleware、Origin 规范化和路由测试。
- Web 全局 API/SSE transport、后端档案、候选验证、React Query/SSE 切换刷新和右上角
  切换器。
- Vitest、Playwright、Storybook 及部署/运维文档。

### Out of scope

- Service Worker app-shell、安装 scope 和静态资源缓存策略。
- 节点间控制面传输和非浏览器 API 客户端。

## 需求（Requirements）

### MUST

- CORS 只允许当前集群 `Node.api_base_url` 的精确 HTTPS origin；未知、非 HTTPS、带路径、
  查询或片段的 origin 不得获得 allow headers。
- `/api` 预检必须允许实际使用的 `GET/POST/PUT/PATCH/DELETE`、`Authorization`、
  `Content-Type`、`Accept`，并返回正确的 `Vary`。
- 浏览器一次只使用一个 Primary Backend；候选必须先通过健康、集群身份、API 兼容性、
  CORS/认证验证。
- 不同 `cluster_id` 不得共享 token、候选或缓存档案。
- 切换必须在 mutation 终态、失败或 60 秒未知超时后才提交，且无自动重放。
- 静态请求和 Service Worker 必须继续使用页面原 origin。

### SHOULD

- 当前主后端故障状态应与 `navigator.onLine` 分开显示。
- 切换后应取消旧查询、增加 transport generation、失效并刷新活动查询及 SSE。

## 功能与行为规格（Functional/Behavior Spec）

1. 启动时先用页面 origin 或已验证的持久 Primary Backend 获取 `cluster/info`；确认
   `cluster_id` 后恢复对应 Profile。
2. 打开切换菜单或请求失败时刷新候选；候选来自页面 origin 和已认证 `admin/nodes`，不接受
   手工 URL。
3. 候选检查顺序为公开 health、cluster info、capabilities/version compatibility，再以
   Bearer token 请求受保护节点清单；任一步失败都不保存候选。
4. 选择候选前等待当前 mutation。终态立即解除；60 秒后记录 unknown、解除屏障并允许人工
   切换，原请求继续完成且不重放。
5. 提交选择后持久化 Profile、更新 generation、取消/失效活动查询并重连 SSE；缓存内容保留
   为 stale 直到新主后端成功响应。
6. 页面 origin 的 `/api` 请求由浏览器 transport 重写到 Primary Backend；非 API、静态资源、
   Service Worker 和既有全页节点导航不重写。

## 接口契约（Interfaces & Contracts）

- 后端：动态 CORS middleware 读取当前 `store.list_nodes()`；允许 origin 只由 `api_base_url`
  解析出的规范化 HTTPS origin 组成。
- 前端：`BackendProfile` 至少包含 `clusterId`、`primaryOrigin`、已验证候选及更新时间；
  Profile 存储按 cluster ID 隔离。
- 前端 transport 暴露当前 origin、generation、可订阅状态、候选验证和
  `switchPrimaryBackend`；所有 API fetch 与 SSE 复用该 transport。
- 受保护探测必须在 cluster ID 匹配后才附带 Bearer token；跨 origin 不使用 cookie 或 credential mode。

## 验收标准（Acceptance Criteria）

- Given 当前节点清单含两个 HTTPS origin，When 页面 origin 发起带 Authorization 的 API 请求，
  Then 两个 origin 均可通过预检和实际请求，未知 origin 无 CORS allow headers。
- Given 候选返回不同 `cluster_id`、不兼容 API、认证失败或 CORS 失败，When 管理员选择候选，
  Then 选择被拒绝且 token 不再发送到该 origin。
- Given 当前主后端不可达，When 管理员打开右上角切换器，Then 缓存页面、候选档案和切换入口
  仍可用。
- Given 页面存在未完成 mutation，When 管理员尝试切换，Then 切换被阻止；终态解除，60 秒
  未知超时解除且不重放。
- Given 切换成功，When 新 API 请求或 SSE 建立，Then 它们只发送到新主后端，静态 app shell
  与 Service Worker origin 不变。

## 非功能性验收 / 质量门槛

- Rust：动态 CORS 单元/路由测试、`cargo fmt --check`、`cargo clippy -- -D warnings`。
- Web：transport/profile/barrier Vitest、双 HTTPS origin Playwright、切换器 Storybook 与受控
  视觉证据、lint/typecheck。
- 文档：本 Spec、ADR、Web Context、`docs/ops/README.md`、`AGENTS.md` 同步。

## 实现前置条件（Definition of Ready / Preconditions）

- [x] 主后端手动切换、同集群 token、严格节点 origin allowlist、60 秒 mutation 屏障已
      确定。
- [x] 现有嵌入式 PWA、相对 `/api` 调用、SSE、离线写保护和全页节点导航已完成代码勘察。

## 实现里程碑（Milestones）

- [ ] 动态 CORS 合同与后端测试完成。
- [ ] Web transport、Profile、候选验证和切换屏障完成。
- [ ] AppShell 切换器、缓存/SSE 协调及 UI/E2E 验证完成。
- [ ] 文档同步、质量门禁和 PR 收敛完成。

## 风险 / 假设 / 开放问题

- 浏览器无法可靠地让应用手工读取预检响应；候选验证以带 Authorization 的受保护探测触发
  并验证 CORS 预检。
- 当前节点清单短暂缺少页面 origin 时，页面仍可访问同源 API，但该 origin 不会被其他节点
  作为跨域来源放行，直到重新进入注册清单。
