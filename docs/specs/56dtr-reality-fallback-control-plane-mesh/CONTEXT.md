# Reality Fallback Control-Plane Mesh

This glossary defines the routing and failure terms for the Reality fallback control plane.

**Direct Mesh**:
A signed HTTP/2 control-plane request from one XP member to another member's eligible
managed-default Reality endpoint. It is neither a user VLESS session nor Native Reverse.
_Avoid_: user XHTTP reuse, VLESS overlay, Reverse Mesh

**XHTTP-Endpoint Direct Mesh**:
Direct Mesh sent to an eligible XHTTP/Reality endpoint. XP uses its own signed HTTP/2
client; it does not establish VLESS, speak XHTTP, or share a client session.
_Avoid_: XHTTP tunnel, user-session reuse

**Public Path**:
A signed HTTPS request to a peer's registered `api_base_url`. It is the bounded fallback
for a Direct Mesh transport failure and the only control-plane path while the durable
cluster Mesh gate is disabled.
_Avoid_: anonymous public API, external primary path

**Direct-Path Validation**:
The directed runtime proof that a sender receives a valid signed `health-v2`
acknowledgement over HTTP/2 from a peer's eligible Reality endpoint. Local canary readiness,
TCP reachability, and a public-path acknowledgement do not establish this proof.
_Avoid_: listener check, endpoint configuration, public fallback success

**Protocol Rejection**:
A response that reaches the HTTP layer but cannot be accepted as the requested signed
control-plane response, including a missing or invalid acknowledgement. It is distinct from
a DNS, TLS, connect, or timeout transport failure.
_Avoid_: weak-network timeout, successful signed error response

**Peer Public Circuit**:
Node-local protection that bounds requests to one peer's Public Path after a public
transport failure or Protocol Rejection. It is not Raft state and does not alter the
cluster-wide Mesh gate.
_Avoid_: cluster disable switch, membership change, Direct Mesh breaker

**Direct Ingress Contract**:
The requirement that an eligible managed XHTTP/Reality endpoint carries Direct Mesh through
the node's TLS canary reserved route on that same endpoint. It adds no port, hostname, XP-local
proxy, VLESS overlay, or user-session reuse.
_Avoid_: second control-plane endpoint, separate tunnel, user ingress contract

**Mesh Re-enable Preflight**:
The operator-visible, all-voter directed verification performed while the cluster Mesh gate is
off. It proves the Direct Ingress Contract for endpoint-bearing targets and the signed registered
API path for an owner-approved private Docker voter without an endpoint before a durable request
can enable Direct Mesh.
_Avoid_: local canary check, unsigned public-path availability, automatic recovery

**Private-Voter Public Target**:
An owner-approved private Docker voter that intentionally has no managed user endpoint. It remains
part of the voter set and uses its registered `api_base_url` for signed control-plane requests;
other voters with eligible endpoints still use Direct Mesh toward one another.
_Avoid_: skipped voter, external-service primary, managed endpoint fallback

**Peer Isolation**:
The local outcome in which a route's circuit is cooling down. Calls fail locally with a retry
deadline until one signed `health-v2` half-open check is allowed. It is neither a node removal
nor a global Mesh disable.
_Avoid_: offline member, automatic failover, permanent blacklist

**Configured Unverified**:
The per-peer state after membership or eligible endpoint metadata changes and before its
required directed Direct-Path Validation succeeds. The affected peer uses the Public Path while
other validated peers may retain Direct Mesh.
_Avoid_: unavailable member, globally disabled Mesh, Direct Mesh success
