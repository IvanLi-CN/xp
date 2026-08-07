# Web 运行时韧性演进历史

> 这里记录影响长期理解的关键演进；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-08-05: 将框架错误恢复与 PWA/API 韧性拆成两个独立 Ticket。
- 2026-08-05: PWA 缓存与 API 兼容同属第二个 Ticket，但保持独立实现和验收，不建立版本绑定。
- 2026-08-05: framework recovery child PR #228 与 PWA/API compatibility child PR #229
  通过各自门禁并合入 Initiative integration branch，进入 aggregate validation。
- 2026-08-05: Issue #227 使用 `injectManifest` 交付构建版本化 app-shell；worker 安装完整性、waiting
  更新、跨标签页 ownership 和延迟清理均独立于 React Query 持久化缓存。
- 2026-08-05: API 兼容窗口固定为 3.22/3.21/3.20；能力、正式 release tag 和本地 fingerprint
  依次探测，release inventory 记录不可变 source commit、路由、调用点和响应字段合同。
- 2026-08-05: API 支持窗口固定为当前及前两个 minor，并覆盖双向兼容。
- 2026-08-05: 完整 API inventory 固定为 pinned source digest 与 callsite method/path/status/capability
  并集；真实 wire-body schema fixtures 聚焦关键页面，禁止用合成空 body 制造全量覆盖假象。
- 2026-08-06: 线上页面实证为 legacy Workbox controller、已完成的 XP waiting app-shell 与无 ownership
  并存。该组合无法显示更新提示，形成 Worker 更新死锁。
- 2026-08-06: 仅对精确 same-scope legacy Workbox cache 引入后台 `skipWaiting()` 迁移例外；它不
  `clients.claim()`、不刷新旧页，并在所有存活页面声明有效 XP build 后仅回收 migration state 记录的 cache。
- 2026-08-06: production preview 覆盖 AppShell、VersionIndicator 浮层及迁移后的当前 build，未复现
  React #185；因此未进行猜测性的 Radix 或组件状态重写。
- 2026-08-07: 确认升级 start 响应直接进入终态时，AppShell 的状态同步 effect 调用 mutation `reset()` 会
  通过 TanStack external store 触发自身重渲染，导致 React #185。同步路径改为只观察 durable upgrade status，
  并以 `succeeded`、`failed`、`unsupported` 三个端到端场景防回归。
- Service Worker 注册后的首次更新检查不能等待周期计时器；立即检查缩短旧 bundle 发现 waiting worker
  的窗口，同时维持用户确认激活的普通更新合同。

## Key Reasons / Replacements

- 已打开的旧 PWA bundle 在服务端升级后触发 React runtime error，暴露了错误恢复、资源一致性和 API 兼容三个不同边界。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
