---
title: Admin PWAs should separate shell caching from authenticated data caching
module: web
problem_type: offline-operations-console
component: admin-pwa
tags:
  - pwa
  - offline
  - react-query
  - sse
status: active
related_specs:
  - docs/specs/7qj3h-web-pwa-offline-console/SPEC.md
---

# Admin PWAs should separate shell caching from authenticated data caching

## Context

The `xp` admin UI needs warm-load offline access after a device has already opened the app
successfully once. Operators should still be able to inspect recent node, alert, and upgrade state
when the upstream service or current network path is degraded.

## Symptoms

- A plain SPA manifest exists, but the browser still treats the app as a normal website.
- Refreshing while offline falls back to a browser error page or a generic in-app fetch failure.
- Authenticated pages depend on live queries, so an otherwise healthy cached UI shell still renders
  blank or unusable after the network drops.
- Realtime status badges go stale unless every page keeps polling independently.

## Root cause

Treating “PWA support” as just a manifest file ignores three separate concerns:

- Static shell availability: HTML, JS, CSS, icons, and SPA navigation fallback.
- Authenticated read-model durability: which query results remain safe and useful to persist.
- Runtime freshness: how cached state gets refreshed while the app is open.

If all three are collapsed into Service Worker runtime caching, auth-protected API responses become
too coarse to reason about. If none are persisted, offline warm-load never becomes useful.

## Resolution

- Use the Service Worker only for static asset precache and SPA navigation fallback.
- Persist only an explicit allowlist of successful React Query read models into IndexedDB, with a
  bounded `maxAge` and a build-version cache buster.
- Keep offline mode explicitly read-only: UI controls disable mutation flows, and a fetch guard
  blocks same-origin mutating `/api/*` requests when offline.
- Feed app-wide operational status through a shared SSE stream, then bridge
  those snapshots back into the persisted React Query cache.
- Reconnect shared SSE streams automatically after backend restarts, and when an
  upgrade job leaves `running`/`restarting`, force a fresh cluster-info read and
  a Service Worker `registration.update()` so the UI can surface a frontend
  reload prompt without waiting for focus or a manual refresh.
- Serve `/sw.js` with stronger no-store semantics than ordinary HTML routes,
  including CDN-facing cache-bypass headers, so an edge cache cannot pin an old
  Service Worker for hours after a release.
- Give every frontend build its own complete app-shell cache. Keep the new
  worker waiting until the operator confirms the update, persist
  `clientId -> buildId` ownership in `xp_sw_metadata`, and delete an old cache
  only after all owners have disappeared. Navigation can use the active build,
  but a controlled page's JS/CSS/font/icon requests must stay on its declared
  build; a cache miss is a recovery signal, never permission to mix builds.
- Treat a legacy Workbox controller as a one-time migration state, not as a normal update. After a
  complete XP app shell is verified, probe undeclared live clients for at most one second. Only when
  the exact same-scope `workbox-precache-v2-<scope>` exists and no client can declare a valid XP
  build may the new Worker call `skipWaiting()`. It must not call `clients.claim()` or reload.
  Persist the exact legacy cache and pre-existing orphan XP app-shell names in `xp_sw_metadata`.
  Delete only those names after every live client has valid XP ownership.
- Keep API compatibility independent from PWA cache identity. The Web client
  checks additive capability IDs first, then a strict stable release tag, then
  a local fingerprint against immutable 3.22/3.21/3.20 inventories. A missing
  declared capability disables only its feature; a 404 or schema failure for a
  declared capability remains a regression signal.
- Classify an initial `401` separately from offline and permission failures:
  offer a re-login link that preserves the current relative location, retain
  the existing token until login verifies a replacement, and do not offer that
  action for `403`.

## Guardrails / Reuse notes

- Do not drop authenticated API responses into generic runtime cache rules by default; make the
  persistence allowlist explicit and reviewable.
- The PWA update prompt should stay separate from server upgrade state. Frontend bundle refresh and
  backend rollout are different operator actions.
- Validate the prompt on the next release after the fix lands. A page that is still running the
  pre-fix bundle cannot discover new Service Worker update behavior just because the backend was
  upgraded underneath it; reload once onto the fixed bundle first, then observe the next version
  change.
- Never use the legacy migration exception for an XP-to-XP update. If any live client has a valid XP
  owner record, preserve the ordinary waiting prompt and explicit user confirmation contract.
- Check the public `sw.js` response headers from the deployed origin, not just the loopback admin
  port. A proxy or CDN that rewrites the Service Worker cache policy can silently break update
  prompts even when the source app returns `no-cache`.
- Offline UX should always surface whether the user is seeing cached data, when it last synced, and
  whether the current view has no local snapshot at all.
- Keep re-authentication recovery in shared error-state and cached-read banner
  primitives so all management reads follow the same 401-only behavior. Initial
  error states retain retry; cached refresh failures retain the useful snapshot
  and expose re-authentication.
- When adding a new admin page, decide whether it belongs to the persisted allowlist and whether its
  writes need extra offline guards before claiming that page supports offline warm-load.

## References

- `web/vite.config.ts`
- `web/src/offline/queryPersistence.ts`
- `web/src/offline/installOfflineApiWriteGuard.ts`
- `web/src/components/AppShell.tsx`
- `src/http/mod.rs`
