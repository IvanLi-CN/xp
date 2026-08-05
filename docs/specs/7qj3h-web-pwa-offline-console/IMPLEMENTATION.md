# Implementation

## Frontend shell and caching

- `web/vite.config.ts` now builds an installable PWA with `vite-plugin-pwa`,
  injects a build-aware cache buster, precaches static assets, and keeps
  navigation fallback scoped to the SPA shell instead of `/api/*`.
- `web/src/main.tsx`, `web/src/vite-env.d.ts`,
  `web/src/offline/queryPersistence.ts`, and
  `web/src/offline/installOfflineApiWriteGuard.ts` register the Service
  Worker, persist allowlisted React Query reads into IndexedDB for `24h`, and
  block offline same-origin API writes.
- `web/src/components/PwaStatusPrompt.tsx` adds a separate UX channel for new
  frontend bundle availability; it does not reuse the backend
  `VersionIndicator` upgrade job wording. The prompt now actively polls the
  registered Service Worker for updates while the app is open.
- The runtime-resilience follow-up uses `injectManifest` with
  `web/src/sw.ts`. Each build is precached under an independent cache name;
  incomplete installs are discarded before activation, the worker remains
  waiting for confirmation, and `xp_sw_metadata` records the build owned by
  each controlled client. Navigation always uses the active build while
  static subresources stay on the declaring client's complete build.

## Offline read-only console

- `web/src/offline/appRuntime.tsx` and `web/src/offline/queryReadState.ts`
  define the shared online / read-only / last-synced state model used across
  pages.
- Major read pages (`HomePage`, `NodesPage`, `NodeDetailsPage`,
  `EndpointsPage`, `EndpointDetailsPage`, `UsersPage`, `UserDetailsPage`,
  `QuotaPolicyPage`, `ServiceConfigPage`, `ToolsPage`) now prefer cached reads
  when possible, surface explicit offline states, and disable
  mutation-oriented controls when offline.
- `web/src/components/ReadStateBanner.tsx` provides the shared
  cached/offline/reconnecting banner primitive, with Storybook coverage for
  snapshot and reconnecting states.

## Authentication recovery

- `web/src/components/AuthRecoveryAction.tsx` classifies structured backend
  errors and renders `Sign in again` only for a `401` failure. It preserves the
  current relative path, query, and hash in the login redirect.
- `PageState`, `ReadStateBanner`, and the Dashboard's direct error cards pass
  their query error to that primitive. Initial failures retain `Retry`; cached
  refresh failures expose recovery from the shared cache banner; `403` remains
  outside the recovery path.
- Node and user detail pages pass their initial and lazy tab-query failures to
  the same primitive, so `Traffic`, `IP usage`, `TCP connections`, and quota
  reads retain both authentication recovery and their existing retry actions.
- Login validation remains the only operation that writes a replacement token;
  a stored token is therefore kept until a new token has been verified. `403`
  errors remain permission failures and do not show the recovery action.

## Status streaming

- `src/http/mod.rs` adds `GET /api/admin/status/events`, an admin-only SSE
  endpoint that emits `hello` plus deduplicated aggregated `snapshot` events
  built from health, cluster info, node runtime, alerts, and upgrade status.
- `web/src/api/adminStatusEvents.ts` consumes the new stream, and
  `web/src/components/AppShell.tsx` bridges incoming snapshots into React Query
  caches so the warm cache remains current while the app is open.
- `web/src/api/sse.ts` now reconnects application SSE streams with bounded
  backoff so admin status snapshots recover after the backend restarts during an
  upgrade.
- When the polled admin upgrade job moves from `running`/`restarting` into a
  terminal state, `AppShell` now forces fresh `clusterInfo` / `health` reads,
  reruns the version check, and asks the registered Service Worker to check for
  a newer frontend bundle immediately.
- `src/http/mod.rs` now serves `/sw.js` with `no-store`-oriented browser and
  CDN cache headers (`Cache-Control`, `CDN-Cache-Control`,
  `Cloudflare-CDN-Cache-Control`, and `Pragma`) so the public edge cannot pin an
  old Service Worker and suppress the update prompt after a release.
- Existing node-detail runtime streaming remains in place; the new admin
  stream covers the shared shell-level status surfaces.

## Coverage and validation

- `src/http/tests.rs` covers the new auth gate and SSE response contract for
  `/api/admin/status/events`, and now also verifies the cache-bypass headers on
  `/sw.js`.
- Web coverage passed through the standard web checks:
  `cd web && bun run lint`
  `cd web && bun run typecheck`
  `cd web && bun run test`
- Storybook coverage includes offline page stories for Nodes and Node details,
  plus component stories for `PageState`, `AuthRecoveryAction`,
  `ReadStateBanner`, and the PWA update prompt card. The unauthorized
  `PageState` story also verifies its login redirect link.
- `AppShell/ApiCompatibilityDegraded` covers feature-local status-stream
  degradation. The API compatibility consumer uses immutable 3.22/3.21/3.20
  release inventories, an additive capabilities probe, strict version fallback,
  and local fingerprint fallback without sharing state with PWA build IDs.
