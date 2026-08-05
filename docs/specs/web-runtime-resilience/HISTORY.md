# Web 运行时韧性演进历史

> 这里记录影响长期理解的关键演进；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-08-05: 将框架错误恢复与 PWA/API 韧性拆成两个独立 Ticket。
- 2026-08-05: PWA 缓存与 API 兼容同属第二个 Ticket，但保持独立实现和验收，不建立版本绑定。
- 2026-08-05: Issue #227 使用 `injectManifest` 交付构建版本化 app-shell；worker 安装完整性、waiting
  更新、跨标签页 ownership 和延迟清理均独立于 React Query 持久化缓存。
- 2026-08-05: API 兼容窗口固定为 3.22/3.21/3.20；能力、正式 release tag 和本地 fingerprint
  依次探测，release inventory 记录不可变 source commit、路由、调用点和响应字段合同。
- 2026-08-05: API 支持窗口固定为当前及前两个 minor，并覆盖双向兼容。

## Key Reasons / Replacements

- 已打开的旧 PWA bundle 在服务端升级后触发 React runtime error，暴露了错误恢复、资源一致性和 API 兼容三个不同边界。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
