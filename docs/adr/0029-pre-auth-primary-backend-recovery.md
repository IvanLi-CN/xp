# Provide pre-auth primary-backend recovery

Status: accepted

The Web primary-backend selector must also be available before admin-token
verification, because a stale or node-local token can otherwise strand the
administrator on a login page served by an unusable node. Pre-auth selection
is limited to the current origin and browser-persisted candidates from the
active cluster profile; the client performs public health, cluster, and
compatibility checks before using a candidate for token verification. It does
not accept arbitrary URLs, discover node inventory anonymously, or introduce
per-node credentials. The choice remains temporary until authentication
succeeds and a fresh authenticated node inventory confirms the selected origin,
after which the existing verified-profile and mutation-barrier contracts
continue to apply. The cached profile reflects a prior inventory and can include
a node that has since been removed; public probes do not prove current membership.

## Considered Options

- Keep selection inside AppShell only: rejected because authentication failure
  prevents reaching the recovery control.
- Allow arbitrary origin entry on the login page: rejected because it weakens
  the exact-origin and same-cluster boundary and creates a credential-forwarding
  surface.
- Publish the full node inventory anonymously: rejected because it exposes
  cluster topology and changes the existing admin API contract.

## Consequences

- A browser with no current origin and no persisted candidate cannot discover a
  new node from the login page; it must use a correct token, a generated login
  link, or a separately configured entrypoint.
- A same-cluster token mismatch remains an operator/deployment error and is
  visible as an authentication rejection rather than silently retried against
  another node.
- Cold-start transport initialization must restore the persisted primary before
  the first authenticated request, with an explicit current-origin fallback.

Related: [web primary-backend spec](../specs/web-primary-backend-failover/SPEC.md),
[ADR 0011](0011-cross-origin-primary-backend-for-embedded-pwa.md).
