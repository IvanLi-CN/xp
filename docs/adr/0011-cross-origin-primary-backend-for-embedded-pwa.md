# Use a manually selected cross-origin primary backend for the embedded PWA

The existing per-node XP Web/PWA remains the browser application in this phase. A
browser profile sends its API and status-stream requests to one manually selected,
verified same-cluster Node origin, while XP continues to perform peer coordination
and forwarding server-side. Every XP node accepts cross-origin console requests only
from the exact HTTPS origins of current registered Nodes; it does not retain
permissive CORS or accept arbitrary operator-entered origins.

## Considered Options

- Independent static console origin: deferred because it requires a separate
  deployment and availability contract beyond this change.
- Browser requests distributed directly across peers: rejected because it expands
  routing, mutation, and authentication behaviour across every caller.
- Same-origin-only console: rejected because an installed PWA needs a usable
  primary-backend failover path when its hosting node is unreachable.

## Consequences

- Backend Profiles are browser-local, scoped to one verified cluster, and retain a
  manually selected Primary Backend plus persisted Candidates.
- Switching is manual. Candidate health is checked only on startup, menu opening,
  explicit selection, and a request failure; requests are never automatically
  replayed on another node.
- An unresolved console write blocks switching until it completes, fails, or times
  out with an explicit unknown-result state. The existing full-page node-navigation
  link remains compatibility behaviour and is not a failover mechanism.
- The PWA's own origin and warm-cache dependency remain unchanged until a separate
  static-console deployment decision is made.
