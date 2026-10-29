# Web 主后端切换实现状态

> 当前合同见 `./SPEC.md`；本文只记录实现覆盖与交付事实。

## Current Status

- Implementation: complete for pre-auth primary-backend recovery
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
  ready, and a successful inventory response replaces stale static rows.
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
  switch attempts are rejected while one barrier is active. A mutation that starts during
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

- The Login page now exposes a same-cluster service-node selector backed by public health,
  cluster, and compatibility probes that never send the admin token.
- On desktop the login selector sits at the top right, with its button and menu sized to the
  widest candidate; on mobile it remains in the login form. A selected node is shown by name
  when available and by host otherwise.
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
- Returning from offline mode forces a fresh public identity probe before protected requests are
  enabled again; a background re-probe for the same identity keeps the active editor mounted.
- A legacy authenticated session without a saved cluster identity clears unbound query data and
  establishes the initial identity only from the current page origin before protected requests.
- Authenticated resource cache identities use stable cluster/origin values rather than the volatile
  in-memory generation, so a same-target reload can recover offline data while an actual target
  change still invalidates the cache; the release compatibility snapshot remains scoped to its
  backend origin for offline fallback.
- An authenticated public probe that proves a different cluster is treated as a credential
  boundary: the token is cleared, the current transport is reset to its page/bootstrap recovery
  origin, and the UI returns to Login before any credentialed request is sent to the new cluster.
  Backend generation changes cancel and remove in-memory and persisted offline query caches, and
  logout resets static policy transport so a later login cannot reuse a stale target.
- The authenticated Home dashboard binds admin-token verification to the starting origin, cluster,
  and backend generation; a node switch during verification cannot persist the token.
- App resource-navigation queries are gated on public backend readiness and include the selected
  cluster and origin in their identity, so stale protected navigation cannot start across a
  backend identity boundary.
- A mutation-barrier switch rechecks the cluster identity and backend generation before committing;
  a target verified for an obsolete identity is rejected instead of becoming primary.
- The authenticated backend switcher remains available while public identity readiness is unresolved
  so a healthy recorded backend can recover the session; each candidate still passes a public probe
  before token-bearing inventory verification, and login-link verification locks the selector, token
  form, and clear action against concurrent manual submissions.
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
- `../../adr/0012-edgeone-static-web-console.md`
- `../../adr/0013-authenticated-node-directory-for-static-console.md`
- `../../adr/0029-pre-auth-primary-backend-recovery.md`
