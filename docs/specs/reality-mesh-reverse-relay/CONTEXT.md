# Reality Mesh Reverse Relay

This glossary separates durable Reverse topology from local liveness and Xray process artifacts.

**Reverse Assignment**:
A Raft-owned mapping from one target to a primary and optional standby Rendezvous. It is
topology, not proof that either network path is presently usable.

**Reverse Link**:
One target-initiated Xray underlay identified by `(epoch, target, rendezvous, role, generation)`.
Primary, standby, and bootstrap are different links even when they serve the same target.
_Avoid_: assignment, tunnel, connection pool.

**Link Lease**:
A target-local, 120-second liveness record granted only after signed `health-v2` returns through
the exact Reverse Link. It is not durable state and does not affect membership.
_Avoid_: Raft lease, endpoint health, leader lease.

**Probe Underlay**:
The one target-side Xray outbound temporarily installed during a 10-second attempt to acquire a
Link Lease. It is removed if no lease returns.
_Avoid_: installation, persistent connection.

**Unverified Link Circuit**:
The acquisition budget for a Reverse Link without a signed health response: one initial 10-second
probe, one recheck after 30 seconds, then a 15-minute cooldown. Each later cooldown permits only
one 10-second half-open probe. A returned signed health replaces this state with a Link Lease.
_Avoid_: retry loop, Xray reconnect policy.

**Open Reverse Circuit**:
The local state after a Reverse Link probe or lease fails. The target has no initiating outbound
for that link until its recheck or cooldown deadline. Direct/Public and membership remain available.
_Avoid_: node down, assignment deletion.

**Healthy Replacement**:
An assignment change in which a current link first acquires a Link Lease, after which an older
link may use the normal 120-second drain. A failed current link is not a healthy replacement.

**Fail-Closed Switch**:
`XP_REVERSE_MESH_ENABLED=false` on one node. It disables only that node's Reverse runtime and
cleans its XP-owned Reverse Xray artifacts; it does not mutate assignments or disable Direct/Public.
