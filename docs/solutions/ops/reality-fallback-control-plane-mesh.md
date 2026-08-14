# Reality Fallback Control-Plane Mesh

## Context

XP control-plane traffic has two independently valid paths to a peer: the managed
VLESS-REALITY endpoint's ordinary HTTPS fallback and the peer's public API origin.
An unavailable response on the preferred path does not prove that a mutation did
not execute. TLS protects the wire, but it cannot answer that application-level
outcome question.

## Decision

- Derive a Mesh origin only from exactly one peer `managed_default` VLESS-REALITY
  endpoint. Treat absent or ambiguous endpoint state as public-only.
- Send signed `health-v2` and `mesh-v2` requests through the canary's reserved
  routes. Normal `/generate_204` and authority-based camouflage requests never
  share that forwarding path.
- Bind v2 signatures to method, raw URI, content metadata, actual body hash,
  cluster/sender/target IDs, request ID and issued-at. Derive distinct request
  and acknowledgement keys from the parsed CA key and certificate fingerprint.
- Reuse a stable request ID when a side-effecting operation can retry across
  paths. Persist the first outcome for the bounded retention window; a caller
  without that guarantee must surface `outcome_unknown` rather than blindly
  fall back.
- Count a signed acknowledgement for any HTTP status as authoritative. Only
  retryable transport failures can open the per-peer breaker or permit a public
  fallback.
- A relay timeout is just as ambiguous as a Mesh timeout. Retry it on a direct public path only
  when the operation is read-only, Raft-idempotent, or protected by the durable request ledger.
- Persist the latest per-peer diagnostic reason independently of the active path. Static target
  reasons are `missing_endpoint`, `ambiguous_endpoint`, and `invalid_access_host`; runtime reasons
  are `no_sample`, `transport_timeout`, `transport_error`, `protocol_rejected`, and
  `fallback_active`. A successful signed Mesh acknowledgement records `mesh_available`.
- The status API treats `mesh_capability` and `mesh_reason` as additive fields. Older snapshots may
  omit them; clients display `unknown` rather than rejecting the snapshot.
- Build the strict Mesh client once per process. Keep it HTTP/2-only with one idle connection per
  origin and a bounded idle timeout; public direct and relay fallback use separate compatibility
  clients. This limits steady-state control-plane sockets without forcing the public fallback onto
  a stricter transport contract.
- Use the HTTP/2 adaptive flow-control window for Mesh instead of reserving a
  large fixed window for every peer. An active Raft snapshot then gets enough
  credit to share a connection with a long-lived stream, while idle peers do
  not retain a snapshot-sized resident buffer.

## Operational Consequences

- A Mesh failure followed by public success is end-to-end availability, but must
  remain visible as fallback in local telemetry and the uptime strip.
- Public standby probes add availability data but do not rewrite the active transport selected by
  real Mesh or public-fallback traffic.
- A 3-minute sample gap is stale telemetry, not evidence that a peer is down.
- No local control-plane proxy participates in peer traffic. Reality Mesh and the peer public
  HTTPS origin are the supported direct paths.
- Clusters change auth epochs only in a maintenance window. Existing v1 `xp-ops` binaries cannot
  parse a new cutover flag, so host-managed cutover bootstraps from a verified target-release
  binary. Container cutover uses a target-image marker command against the persistent volume. The
  marker is cancellable only before the first v2 process consumes it into the durable epoch record;
  that durable state intentionally rules out a v1 rollback, including when later runtime reconcile
  work fails.

## Validation Pattern

- Test a completed mutation whose response is lost, then retry it through the
  alternate path with the same request ID; the observed result must be the first
  result across restart.
- Test headers/body/timestamp/target tampering, untrusted member identity,
  and reserved-route requests before the canary forwards any body.
- Test Mesh-only, public-only and dual-path faults separately. The first should
  show fallback; the second should surface a Mesh success with standby failure;
  the third should mark the peer unavailable.
- For a read-only cluster audit, collect the managed endpoint count, validated access host, canary
  readiness, Xray listener, DNS/port reachability, and signed `health-v2` acknowledgement for every
  directed edge. Do not change configuration, restart services, reset breakers, or infer Mesh
  capability from a public fallback success alone.
- Exercise the reserved Canary route over both HTTP/1.1 and negotiated HTTP/2. HTTP/2 servers may
  retain scheme and authority in the incoming URI; never append that complete URI to a loopback
  origin. Rebuild the upstream URL from the fixed loopback origin plus the original path/query, and
  assert that percent encoding and query ordering remain unchanged because internal-auth v2 signs
  those bytes. If the HTTP client would normalize those bytes, reject the request instead of
  forwarding a URI that no longer matches the authenticated canonical value.
- In a candidate-versus-baseline resource comparison, use separate Cargo target directories. A
  shared target can reuse a candidate build script or generated artifact under the baseline source,
  making PSS, CPU, and connection comparisons invalid before the workload begins.
