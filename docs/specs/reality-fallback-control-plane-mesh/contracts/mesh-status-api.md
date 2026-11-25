# Mesh Status API Contract

## `GET /api/admin/mesh/status`

- Requires administrator Bearer authentication.
- `cluster_mesh_enabled` is the durable cluster-wide Mesh setting from the
  replicated state. A fresh non-bootstrap node may report `true` here while its
  local admission gate is still closed until the first authenticated state or
  snapshot apply; peer paths remain public during that bootstrap window.
- Returns local node state, remote peer routes, availability, breaker, RTT,
  stale state, 24h buckets and local events.
- Additive peer fields expose `direct_validation` and `public_circuit`. The former is
  `configured_unverified`, `verified`, `transport_failed`, or `protocol_rejected`; the latter
  uses the existing `closed`, `open`, `half_open`, or `disabled` breaker states. They are path
  facts from the current node, not cluster membership or user-inbound state.
- `direct_connectivity` is the latest Direct Connectivity Probe outcome: `verified`, `unavailable`,
  `not_applicable`, or `unknown`. It is the authoritative current Direct health signal; status and
  Web MUST use it rather than stale `direct_validation`. It is separate from the peer quality enum.
- A Direct or Public probe MUST NOT update `direct_validation`; only explicit enablement/admission
  validation may do so. `direct_validation` remains diagnostic and must not mask
  `direct_connectivity`.
- `public_fallback_health` is the latest Public Fallback Probe outcome: `available`, `unavailable`,
  or `unknown`. It is probe-only; functional public fallback remains `fallback_success`.
- Peer quality is good, slow, unstable, down or unknown.
- `current_path` reflects functional routing evidence only; a public fallback probe cannot update
  it. A functional public fallback updates `current_path` and is recorded in bucket
  `fallback_success`, which is not Direct Connectivity evidence.
- Direct Connectivity and Public Fallback health are separate facts. A public fallback probe success
  must never make a peer's Direct path appear healthy or overwrite its active path.
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
- Probes are Direct-only. A member without an eligible Direct Mesh path is not dispatched: it is
  listed in `not_applicable_node_ids`, is absent from `accepted_node_ids`, and causes no public
  request. `accepted_node_ids` contains only targets for which a Direct probe was dispatched.
- The scheduled public fallback probe is separate, public-only, and runs every five minutes; it
  writes `public_fallback_health`, not `fallback_success`, and does not update `current_path` or
  `direct_validation`.
- Returns `accepted_node_ids`, `not_applicable_node_ids` and the telemetry revision.

## Web consumer

- `/system-status` displays all remote peers, refresh, probe-all, probe-one
  and node-detail navigation.
- Offline mode may render the timestamped persistent snapshot.
- Offline mode disables probes.
- New Web clients hide reuse details when the capability or optional object is absent. When
  present but unsampled, the peer row displays `Reuse data unavailable`; otherwise it displays
  `H2 · N req / M starts · gen G` inline with the current path.
- New Web clients render `direct_connectivity` and `public_fallback_health` as separate status
  facts; a public `available` state must never render the Direct path as `verified`.

## `PUT /api/admin/mesh/config`

- Requires administrator Bearer authentication and a JSON body of
  `{ "enabled": boolean }`.
- Writes the cluster-wide Mesh setting through Raft. Before a new mutation is
  accepted, every current voter and learner must expose the required Mesh
  capability.
- When `enabled=true`, the leader additionally runs a route-qualified `health-v2` preflight for
  every directed current-voter edge while the gate remains closed. Targets with an eligible managed
  endpoint use Direct-only; an owner-approved private Docker target without that endpoint uses its
  registered `api_base_url`. A failed preflight returns HTTP `409`
  with `error.code=mesh_preflight_failed` and bounded `failures[]` entries containing only
  `sender_node_id`, `target_node_id`, and `kind`; it does not write Raft state.
- Disabling Mesh keeps existing public paths available. Enabling only takes
  effect for new Mesh attempts after the replicated state is applied locally.

The signed internal `POST /api/admin/_internal/raft/client-write` path may carry
an idempotency request. An existing signed request replays its stored result
before the Mesh capability check; the public Bearer-authenticated PUT above has
no idempotency header contract.
