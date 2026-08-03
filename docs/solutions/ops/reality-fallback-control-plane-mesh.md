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

## Operational Consequences

- A Mesh failure followed by public success is end-to-end availability, but must
  remain visible as fallback in local telemetry and the uptime strip.
- A 3-minute sample gap is stale telemetry, not evidence that a peer is down.
- `XP_MESH_PROXY_URL` remains a public-egress proxy-first/direct compatibility
  layer; it does not carry Reality Mesh traffic.
- Clusters change auth epochs only in a maintenance window. The explicit
  cutover marker is consumed at startup into a durable epoch record so a Web
  single-node upgrade cannot silently create a mixed cluster.

## Validation Pattern

- Test a completed mutation whose response is lost, then retry it through the
  alternate path with the same request ID; the observed result must be the first
  result across restart.
- Test headers/body/timestamp/target tampering, untrusted member identity,
  and reserved-route requests before the canary forwards any body.
- Test Mesh-only, public-only and dual-path faults separately. The first should
  show fallback; the second should surface a Mesh success with standby failure;
  the third should mark the peer unavailable.
