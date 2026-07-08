# History

- 2026-07-08: Added installable PWA shell, IndexedDB-backed offline read
  cache, admin aggregated status SSE, and explicit offline read-only UX across
  the major Web management pages.
- 2026-07-09: Fixed post-upgrade frontend refresh signaling by reconnecting the
  admin SSE stream after backend restarts, refreshing shell status when the
  upgrade job reaches a terminal state, and proactively polling the registered
  Service Worker for newer bundles.
