# Web 主后端切换实现状态

> 当前合同见 `./SPEC.md`；本文只记录实现覆盖与交付事实。

## Current Status

- Implementation: complete
- Lifecycle: active
- Delivery: fast-track

## Planned Surfaces

- Rust HTTP：动态浏览器 CORS middleware 与覆盖测试。
- Web：`src/backend/` transport/profile/provider，AppShell 右上角切换器，
  query/SSE/offline integration。
- Validation：Vitest、Playwright、Storybook/视觉证据及 Rust/Web quality gates。

## Delivered

- Rust `/api` CORS now derives exact HTTPS origins from the current node inventory and handles
  Authorization preflight without opening static resources.
- Web transport rewrites API and SSE requests to one verified primary origin, persists profiles by
  cluster ID, preserves offline cache, and enforces the mutation switch barrier.
- AppShell exposes the primary backend switcher and refreshes active queries/status events after a
  successful manual switch; the existing full-page node handoff remains unchanged.
- Static Web mode uses `https://101-xp.ivanli.cc` as its bootstrap origin. After the
  Bearer-protected runtime-policy route admits the current backend, the console reads the
  authenticated registered node inventory and renders real node names and origins; the policy
  remains the independent switching gate and anonymous pages never receive topology. Policy
  refreshes stage new origins without reloading the current document; only an explicit reload
  confirmation activates the staged policy. A temporarily unavailable node remains visible but is
  excluded from the active allowlist. A ten-minute policy grant is held in memory by the Service
  Worker per controlled client and rewrites only the next navigation's `connect-src`.
- Static Web directory queries wait for a matching, unexpired Runtime Policy. Service-version
  reads are enabled only while the selector is open, refresh once on open, and do not refetch on
  mount, reconnect, or window focus. Explicit refreshes are also blocked until that policy gate is
  ready, and a successful inventory response replaces stale static rows. Each open clears prior
  version labels, and only results from that refresh can restore them.
- Selector rows are keyed by registered node ID, independently of the origin-keyed runtime
  policy. Policy updates annotate existing rows without retiring them. Within the authenticated
  session a failed directory refresh retains the last successful rows and shows one stale notice;
  a first failure shows an error. The selector uses a non-persisted query and a session epoch plus
  request ordering to reject responses from a previous login, cluster or older request. A
  successful empty inventory retires every previously registered row.
- The selector keeps switching and full-page node opening as separate actions. Node opening uses
  the existing path/query/hash and login handoff, targets a new tab with `noopener noreferrer`, and
  uses a fragment-only handoff for selector links; the destination login route consumes and replaces
  that fragment before navigation. It disables invalid or non-canonical HTTPS targets. A bounded
  authenticated version fan-out reads
  each node's local release through the fixed `/api/cluster/info` route, verifies the cluster ID,
  and reports only service versions that differ from the recognized frontend release. Unknown
  service versions and unrecognized frontend releases omit the comparison label entirely.
  The bounded fan-out shares one aggregate deadline across all registered nodes.
- The mutation barrier rechecks for a mutation opened during drain notification before committing
  a primary switch, and the selected candidate is revalidated against a fresh authenticated
  directory and, for Static Web, Runtime Policy before commit. Failed revalidation restores the
  ready state; cached data from an unsuccessful refetch never satisfies the check, and concurrent
  switch attempts are rejected while one barrier is active. A candidate removed or made unavailable
  by the refreshed policy receives no subsequent authenticated inventory request. When a newer
  directory refresh fails, a successful candidate revalidation can still verify a node already in
  the retained inventory without replacing that inventory. A mutation that starts during
  revalidation is drained before commit or rejects the switch when the initial barrier has not
  timed out; after the initial unknown timeout, manual switching remains available. Policy
  hydration preserves the active switching state until the commit path finishes.
- Registered HTTPS origins reject URL userinfo at both metadata validation and outbound URL joining,
  so credentials cannot be supplied through a node address.
- External subscription URLs are built from the current verified Primary Backend origin, so copied
  URLs remain directly consumable by clients that do not execute the Web app runtime.
- Vite emits the build declaration and document-fallback handler as a content-hashed external asset.
  The release build packages `web/dist` with `edgeone.json`, a same-SHA manifest, and checksum
  before
  the static EdgeOne deployment gate. The image and GitHub Release jobs depend on the public static
  smoke gate; the protected static rollback workflow reuses only verified release archives.

## References

- `./SPEC.md`
- `./HISTORY.md`
- `../../adr/0011-cross-origin-primary-backend-for-embedded-pwa.md`
- `../../adr/0012-edgeone-static-web-console.md`
