# Node-local Mihomo private CIDR policy

## Decision

Authorize private Mihomo mirror targets with a node-local CIDR policy. The
deployment default is an environment variable and the Web override is an
atomic file in that node's data directory. Remote administration uses the
existing signed Mesh path and does not create replicated Raft state.

## Rationale

Split DNS intentionally resolves selected service names to private addresses.
A cluster-wide boolean either blocks valid internal access everywhere or grants
unbounded private access everywhere. CIDR authorization preserves the network
topology while keeping each node's trust boundary explicit.

## Consequences

Every DNS result and redirect must be revalidated and pinned. Operators must
configure each node independently. A corrupt local override fails closed for
private targets and remains visible through the node policy API.
