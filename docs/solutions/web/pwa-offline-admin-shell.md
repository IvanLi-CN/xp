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

## Guardrails / Reuse notes

- Do not drop authenticated API responses into generic runtime cache rules by default; make the
  persistence allowlist explicit and reviewable.
- The PWA update prompt should stay separate from server upgrade state. Frontend bundle refresh and
  backend rollout are different operator actions.
- Offline UX should always surface whether the user is seeing cached data, when it last synced, and
  whether the current view has no local snapshot at all.
- When adding a new admin page, decide whether it belongs to the persisted allowlist and whether its
  writes need extra offline guards before claiming that page supports offline warm-load.

## References

- `web/vite.config.ts`
- `web/src/offline/queryPersistence.ts`
- `web/src/offline/installOfflineApiWriteGuard.ts`
- `web/src/components/AppShell.tsx`
- `src/http/mod.rs`
