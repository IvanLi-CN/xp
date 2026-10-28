# Read an authenticated registered node directory in the static console

The independent static console may show and open registered cluster nodes after
the administrator has authenticated and received a valid Runtime Policy. The
policy remains the source of truth for which HTTPS origins may be selected; the
node directory supplies names, node IDs and the registered address needed for a
useful selector row. Anonymous bootstrap pages still receive no topology.

The selector uses the existing manual backend verification and mutation barrier
for API switching. A separate per-node link opens the registered frontend in a
new tab, preserving the current path, query and hash and using the existing
login handoff. Selector links carry that handoff in a fragment-only envelope;
the destination login route consumes it and replaces the history entry before
continuing, so the administrator token is not sent in the navigation request
URL. It does not proxy API traffic or make a node link a switching permission.

Service versions are read through an authenticated aggregate endpoint. The
aggregate resolves only Raft-registered node IDs and HTTPS addresses, sends the
fixed `/api/cluster/info` request without forwarding administrator credentials,
rejects redirects, validates the returned cluster ID and node ID, bounds each
response and fan-out, and reports an unknown result when an older or unavailable
node cannot answer.
The Web client compares only a recognized formal frontend release version and
shows a service version only when it differs.

## Consequences

- ADR 0012's anonymous static-console boundary remains intact, while its
  post-authentication policy path now has a usable node directory.
- Topology is not broadened to arbitrary URLs, cross-cluster profiles,
  automatic failover, background polling, or a general API proxy.
- Old nodes remain usable for inventory, switching and frontend opening even
  when they do not implement the version endpoint; only their version label is
  unknown.
- The selector has two explicit keyboard actions per valid row: select the API
  backend and open the node frontend. Invalid HTTPS addresses disable only the
  latter and expose a reason.
