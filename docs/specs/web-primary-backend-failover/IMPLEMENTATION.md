# Web 主后端切换实现状态

> 当前合同见 `./SPEC.md`；本文只记录实现覆盖与交付事实。

## Current Status

- Implementation: in progress
- Lifecycle: active
- Delivery: fast-track

## Planned Surfaces

- Rust HTTP：动态浏览器 CORS middleware 与覆盖测试。
- Web：`src/backend/` transport/profile/provider，AppShell 右上角切换器，query/SSE/offline integration。
- Validation：Vitest、Playwright、Storybook/视觉证据及 Rust/Web quality gates。

## References

- `./SPEC.md`
- `./HISTORY.md`
- `../../adr/0011-cross-origin-primary-backend-for-embedded-pwa.md`
