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
