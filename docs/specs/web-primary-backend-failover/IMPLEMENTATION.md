# Web 主后端切换实现状态

> 当前合同见 `./SPEC.md`；本文只记录实现覆盖与交付事实。

## Current Status

- Implementation: complete for pre-auth primary-backend recovery
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

- The Login page now exposes a same-cluster service-node selector backed by public health,
  cluster, and compatibility probes that never send the admin token.
- Pre-auth selection is session-local, respects the existing mutation barrier, and routes token
  verification plus subsequent relative API/SSE requests through the selected origin.
- Cold-start transport restores a canonical persisted primary origin when it is still present in
  the active profile, while the current page origin remains an explicit recovery candidate.
- A current-origin probe that proves the persisted cluster identity is stale rebinds only the
  current tab to the new cluster and discards old candidates before authentication; the new
  profile remains non-durable until authenticated inventory hydration.
- The authenticated AppShell waits for a current-mount public cluster identity before sending
  token-bearing compatibility traffic; its inventory query is cluster-keyed and only matching
  inventory can hydrate or persist the active profile. All token-bearing inventory requests also
  require that same origin-and-cluster public-readiness gate.
- An authenticated public probe that proves a different cluster is treated as a credential
  boundary: the token is cleared, the current transport is reset to its page/bootstrap recovery
  origin, and the UI returns to Login before any credentialed request is sent to the new cluster.
  Backend generation changes cancel and remove in-memory and persisted offline query caches, and
  logout resets static policy transport so a later login cannot reuse a stale target.
- The authenticated Home dashboard binds admin-token verification to the starting origin, cluster,
  and backend generation; a node switch during verification cannot persist the token.
- App resource-navigation queries are gated on public backend readiness and include the selected
  origin and generation in their identity, so stale protected navigation cannot start across a
  backend identity boundary.
- A mutation-barrier switch rechecks the cluster identity and backend generation before committing;
  a target verified for an obsolete identity is rejected instead of becoming primary.
- Login-page probe completions are epoch-bound so a probe from a discarded cluster cannot mark a
  candidate ready after a stale-profile rebind.
- Login verification responses are bound to the starting origin, cluster identity, and backend
  generation; selector interaction is disabled while verification is in flight.
- Successful authenticated node-inventory hydration is the persistence gate for a new primary;
  authentication rejection keeps the login page and unverified token out of local storage.
- Login submission is gated on the selected target's public probe and the existing mutation
  barrier; static-console login links keep the bootstrap transport and failed probes remain
  retryable without dropping candidates.
- Vitest, Storybook interaction, Playwright browser, and the cluster join token-hash regression
  coverage are recorded in the specification verification matrix.

The remaining work for this topic is governed by the repository delivery flow and the live PR
checks; no server-side authentication protocol or deployment behavior changed.

## References

- `./SPEC.md`
- `./HISTORY.md`
- `../../adr/0011-cross-origin-primary-backend-for-embedded-pwa.md`
- `../../adr/0028-pre-auth-primary-backend-recovery.md`
