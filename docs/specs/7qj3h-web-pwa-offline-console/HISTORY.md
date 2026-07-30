# History

- 2026-07-08: Added installable PWA shell, IndexedDB-backed offline read
  cache, admin aggregated status SSE, and explicit offline read-only UX across
  the major Web management pages.
- 2026-07-09: Fixed post-upgrade frontend refresh signaling by reconnecting the
  admin SSE stream after backend restarts, refreshing shell status when the
  upgrade job reaches a terminal state, and proactively polling the registered
  Service Worker for newer bundles.
- 2026-07-09: Hardened `/sw.js` delivery with browser and CDN cache-bypass
  headers after live validation showed the public edge caching the Service
  Worker for four hours and suppressing the frontend update prompt.
- 2026-07-30: Added shared 401 authentication recovery to initial management
  read failures, preserving the failed route for verified re-login while
  keeping 403 as a permission error.
