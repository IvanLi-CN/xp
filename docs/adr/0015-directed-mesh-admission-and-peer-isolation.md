# Require Directed Mesh Admission and Isolate Failed Peer Paths

Status: accepted

XP keeps Direct Mesh as the cluster-first path for an eligible managed-default Reality endpoint,
including XHTTP Reality fallback. Direct Mesh is one signed HTTP/2 request to the existing
endpoint's TLS canary reserved route; it does not add a hostname, port, XP-local proxy, VLESS
overlay, or user-session reuse. Native Reverse remains quarantined and is not a recovery option.

The durable `mesh_enabled` value records only the cluster's routing intent. Enabling it requires
a bounded, server-enforced re-enable preflight: each current voter performs a Direct-only signed
`health-v2` request to every other current voter. The preflight runs while the gate remains
disabled, has no Public fallback, is limited to 30 seconds per enable operation with at most
four concurrent directions per node, and succeeds only when every directed edge returns an HTTP/2
response with a valid signed acknowledgement. A failed edge rejects the enable request without a
Raft write and identifies only the peer pair and classified failure; it does not expose socket,
address, certificate, or user traffic data.

Each node retains local path evidence and two separate circuit concerns. The Direct circuit keeps
its bounded transport failure policy. A new Peer Public Circuit prevents a failed Public Path
from becoming an unbounded Raft retry source: DNS, TLS, connect, and timeout failures consume the
bounded request retry budget before contributing a failure; an unsigned or invalid acknowledgement
is a Protocol Rejection and isolates the Public Path immediately. Both circuit types use
`30/60/120/240/300s` cooldowns. During half-open, exactly one side-effect-free signed
`health-v2` request may reach the peer; concurrent requests fail locally with a retry deadline.
A valid signed acknowledgement restores only the proven path. Circuits and receipts are local,
bounded telemetry, never Raft state.

Endpoint or membership changes invalidate only the affected peer's Direct-Path Validation. That
peer becomes `configured_unverified` and uses the Public Path; validated peers remain eligible
for Direct Mesh. XP never auto-disables the cluster gate, removes a member, rewrites endpoint
metadata, or enables Native Reverse as a consequence of a path failure.

## Considered Options

- Permanent Public-only routing was rejected because it makes an externally routed endpoint the
  primary dependency even when a cluster-owned Direct path is healthy.
- A VLESS/XHTTP overlay or user-session reuse was rejected because it would recreate the dynamic
  Xray lifecycle and resource boundary already rejected for Native Reverse.
- Manual enable checklists were rejected because they can validate only one direction and miss
  leader changes or asymmetric cross-region reachability.
- Replicating receipts and circuit state through Raft was rejected because reachability is a
  node-local, time-sensitive observation rather than cluster configuration.
- Automatically turning off Mesh after one peer failure was rejected because it broadens a
  single-peer fault into a cluster-wide routing change.

## Consequences

- The re-enable operation has a bounded distributed preflight and may return a structured
  conflict instead of writing `mesh_enabled=true`.
- System Status and the admin API distinguish endpoint configuration, directed Direct validation,
  Public Path health, peer isolation, and active route. External user inbound counts remain
  separate from control-plane transport evidence.
- Release validation must run actual XP and Xray from each voter direction under normal and weak
  network conditions, and must demonstrate bounded PSS, CPU, and control-plane connections when
  a peer returns no signed acknowledgement.
- Deploying the remediation and re-enabling Mesh are separate owner-authorized operations. The
  gate remains disabled until the re-enable preflight and resource acceptance pass.
