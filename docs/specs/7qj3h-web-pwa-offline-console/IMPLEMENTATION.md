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
  `VersionIndicator` upgrade job wording.

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

## Status streaming

- `src/http/mod.rs` adds `GET /api/admin/status/events`, an admin-only SSE
  endpoint that emits `hello` plus deduplicated aggregated `snapshot` events
  built from health, cluster info, node runtime, alerts, and upgrade status.
- `web/src/api/adminStatusEvents.ts` consumes the new stream, and
  `web/src/components/AppShell.tsx` bridges incoming snapshots into React Query
  caches so the warm cache remains current while the app is open.
- Existing node-detail runtime streaming remains in place; the new admin
  stream covers the shared shell-level status surfaces.

## Coverage and validation

- `src/http/tests.rs` covers the new auth gate and SSE response contract for
  `/api/admin/status/events`.
- Web coverage passed through the standard web checks:
  `cd web && bun run lint`
  `cd web && bun run typecheck`
  `cd web && bun run test`
- Storybook coverage includes offline page stories for Nodes and Node details,
  plus component stories for `PageState` and `ReadStateBanner`.
