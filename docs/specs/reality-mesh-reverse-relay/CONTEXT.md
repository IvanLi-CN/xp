# Reality Mesh Reverse Relay

This glossary separates durable Reverse topology from local liveness and Xray process artifacts.

**Direct Mesh**:
A signed control-plane path from one XP member directly to a peer's eligible Reality endpoint.
It is distinct from both Reverse Relay and the peer's public HTTPS endpoint.
_Avoid_: Reverse Mesh, public direct

**XHTTP-Endpoint Direct Mesh**:
Direct Mesh that uses the Reality fallback of a managed XHTTP endpoint. XP sends its existing
signed HTTP/2 control-plane protocol; it does not wrap that traffic in VLESS/XHTTP or share a
user's XHTTP session.
_Avoid_: VLESS/XHTTP overlay, user XHTTP reuse

**Eligible Reality Endpoint**:
The one managed-default VLESS/Reality endpoint for a node with a valid `access_host`. It is
eligible for Direct Mesh from durable configuration; a signed acknowledgement, breaker and
telemetry establish its runtime reachability.
_Avoid_: canary precondition, operator URL probe

**Cluster-First Control Plane**:
The routing policy that attempts Direct Mesh before public HTTPS. Public HTTPS is a bounded
fallback, not the cluster's primary dependency; this policy does not require Reverse Relay.
_Avoid_: public-only, Reverse-first, external-first

**Reverse Assignment**:
A Raft-owned mapping from one target to a primary and optional standby Rendezvous. It is
topology, not proof that either network path is presently usable.

**Reverse Link**:
One logical Xray lifecycle is identified by `(epoch, target, rendezvous, role, generation)`.

A Link may use more than one physical underlay connection, but primary, standby,
and bootstrap are different links even when they serve the same target.
_Avoid_: assignment, tunnel, connection pool.

**Reverse Underlay Connection**:
One physical TCP connection opened by Xray's XMUX for a single Reverse Link. It is a transport
resource count, not a user session and not another Reverse Link.
_Avoid_: user inbound session, logical link, node connection.

**Native Reverse**:
The XP-managed Xray dynamic Reverse implementation using a target-side VLESS outbound and a
Rendezvous SOCKS portal. It is excluded from the cluster-first request route until physical
underlay closure and budgeting can be enforced and verified.
_Avoid_: Direct Mesh, public fallback

**Dormant Native Reverse**:
The retained Native Reverse code and persisted topology while it is prevented from reconciling,
probing, creating Xray artifacts or serving requests. It is a diagnostic state, not a fallback.
_Avoid_: enabled standby, automatic recovery

**User Inbound Session**:
A client-initiated VLESS session accepted by a managed node endpoint. It is independent of
Reverse underlay connections and must be counted separately in resource investigations.
_Avoid_: Reverse session, underlay connection.

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
