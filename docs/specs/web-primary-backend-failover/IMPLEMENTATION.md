# Web 主后端切换实现状态

> 当前合同见 `./SPEC.md`；本文只记录实现覆盖与交付事实。

## Current Status

- Implementation: existing contract complete; pre-auth recovery gap not started
- Lifecycle: active
- Delivery: fast-track

## Planned Surfaces

- Rust HTTP：动态浏览器 CORS middleware 与覆盖测试。
- Web：`src/backend/` transport/profile/provider，AppShell 右上角切换器，query/SSE/offline integration。
- Validation：Vitest、Playwright、Storybook/视觉证据及 Rust/Web quality gates。

## Delivered

- Rust `/api` CORS now derives exact HTTPS origins from the current node inventory and handles
  Authorization preflight without opening static resources.
- Web transport rewrites API and SSE requests to one verified primary origin, persists profiles by
  cluster ID, preserves offline cache, and enforces the mutation switch barrier.
- AppShell exposes the primary backend switcher and refreshes active queries/status events after a
  successful manual switch; the existing full-page node handoff remains unchanged.
- Static Web mode uses `https://101-xp.ivanli.cc` as its bootstrap origin and never reads the
  authenticated node inventory. The Bearer-protected runtime-policy route supplies only canonical
  compatible API origins; a ten-minute policy grant is held in memory by the Service Worker per
  controlled client and rewrites only the next navigation's `connect-src`.
- Vite emits the build declaration and document-fallback handler as a content-hashed external asset.
  The release build packages `web/dist` with `edgeone.json`, a same-SHA manifest, and checksum
  before
  the static EdgeOne deployment gate. The image and GitHub Release jobs depend on the public static
  smoke gate; the protected static rollback workflow reuses only verified release archives.

## Remaining Gap

- The Login page does not yet expose pre-auth backend recovery.
- Cold-start transport initialization does not yet restore the persisted
  `primaryOrigin` before the first authenticated request.
- Login-page Storybook and browser coverage for selected-target authentication,
  401 recovery, stale candidates, and zero-candidate fallback is not yet added.

The requirements and acceptance contract for this work are recorded in
`SPEC.md` and ADR 0028. No implementation is claimed by this document.

## References

- `./SPEC.md`
- `./HISTORY.md`
- `../../adr/0011-cross-origin-primary-backend-for-embedded-pwa.md`
- `../../adr/0028-pre-auth-primary-backend-recovery.md`
