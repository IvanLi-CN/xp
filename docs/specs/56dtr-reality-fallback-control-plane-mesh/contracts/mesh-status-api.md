# Mesh Status API Contract

## `GET /api/admin/mesh/status`

- Requires administrator Bearer authentication.
- Returns local node state, remote peer routes, availability, breaker, RTT,
  stale state, 24h buckets and local events.
- Peer quality is good, slow, unstable, down or unknown.
- `current_path` and bucket `fallback_success` distinguish public fallback.
- Computes ETag from the complete stable response representation (without its generated-at time)
  and returns `304 Not Modified` for a matching `If-None-Match`.
- Public health does not expose URL, signatures or fallback diagnostics.
- A peer may include `mesh_transport` when `admin.mesh-transport-reuse` is declared:
  - `protocol`: `h2` when the latest successful Mesh response used HTTP/2.
  - `health`: `unknown`, `healthy`, or `churning`.
  - `connection_generation` and `current_connection_requests`.
  - `requests_5m`, `connection_starts_5m`, `requests_1h`, and `connection_starts_1h`.
  - optional `last_connection_started_at`.
- `mesh_transport` never contains a URL, IP address, socket address, port, certificate identity,
  or connection fingerprint. The fingerprint is process-local and never serialized.
- The complete `mesh_transport` representation participates in the status ETag. Old telemetry
  decodes without these fields, and old servers may omit the object entirely.

## `POST /api/admin/mesh/probes`

- Requires administrator Bearer authentication.
- Optional `node_ids` selects remote current members; omission probes all peers.
- Rejects local, unknown, duplicate and more than 50 node IDs.
- All remote targets are derived from replicated member and endpoint state.
- Returns accepted peer IDs and the telemetry revision.

## Web consumer

- `/system-status` displays all remote peers, refresh, probe-all, probe-one
  and node-detail navigation.
- Offline mode may render the timestamped persistent snapshot.
- Offline mode disables probes.
- New Web clients hide reuse details when the capability or optional object is absent. When
  present but unsampled, the peer row displays `Reuse data unavailable`; otherwise it displays
  `H2 · N req / M starts · gen G` inline with the current path.
