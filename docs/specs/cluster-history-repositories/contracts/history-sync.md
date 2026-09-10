# History Sync Contract

## Identity and streams

Every source has a durable Ed25519 identity managed by Raft. A record retains
both `subject_node_id` and `observer_node_id`. Streams are independent:
`runtime`, `path_health`, `traffic`, `connections`, `ip_usage`, and `tombstone`.

The cursor key is `(source_node_id, source_epoch, stream, sequence)`. Sequence
is monotonic within an epoch. A database rebuild, lost sequence, or source fork
creates a new epoch. Cursors never skip holes.

## Segment and envelope

Segments are immutable and close at 1000 records, 192 KiB canonical
uncompressed, or one minute. A segment is never split in transfer. The signed
envelope covers cluster, source, epoch, stream, sequence range, record hash,
previous segment hash, and schema version.

The canonical response is capped at 1 MiB uncompressed and 256 KiB on wire.
Payloads below 4 KiB use identity. Other payloads may use only Zstandard level
1; identity is selected when compression is not beneficial. Receivers enforce
decompressed-size, record-count, nesting and expansion-ratio limits before storage.

## Paths

Reality Mesh and Cloudflare Tunnel are equal-level direct paths. The path
selector prefers a healthy/stable path, switches with hysteresis, and probes
standby at low frequency. Only when both direct paths fail may the source try
the Raft-assigned Reality Mesh Reverse relay; only after that fails may it
attempt a jittered hourly relay through an eligible Mesh member. Both relays
are streaming only and persist no history. The dynamic relay carries end-to-end
X25519 plus AEAD payloads; its batches are compressed and paged against the
actual encrypted-frame budget before sealing.

## Acknowledgement and repair

An acknowledgement advances only a continuous watermark. Expired cursors return
the earliest retained cursor and an explicit gap. Tombstones replicate before
affected records and remain until every current ready repository acknowledges
them plus the tombstone horizon. Anti-entropy exchanges partition summaries,
repairs ranges first, then drills down.

The bounded repair response contains `segments`, `gaps`, and the additive
`unavailable_segment_ids` field. The latter lists only requested 64-character
hex segment IDs that the serving repository no longer retains under the
unchanged retention policy, such as when a summary/repair request crosses the
seven-day minute-tier boundary. It is limited to 64 IDs, must be unique, and
does not carry payload or acknowledgement meaning. A syncing peer may remove
only the exact IDs it requested and received in this field; duplicate, unknown,
malformed, or out-of-page IDs fail closed. Older responses that omit the field
are interpreted as an empty list, preserving wire compatibility.

A temporary transport failure, exhausted retry schedule, or full bounded outbox
creates Recoverable Backlog, never a permanent gap. A Source or any ready
repository that retains the original cursor range may repair it. A permanent
gap is valid only after the original range has expired under the unchanged
source-retention policy and neither the Source nor any ready repository can
supply it.

Internal source-delivery requests authenticate and pin the declared source
identity before applying gap metadata. Recoverable and permanent gaps from that
authenticated source are both accepted, while gaps naming another source or
exceeding the bounded 64-item request limit are rejected. The `permanent` flag
continues to control only whether the receiver may advance past an expired
range; it does not gate delivery of recoverable backlog. When more than 64
recoverable ranges are retained, the source persists a page cursor and rotates
through the full set across delivery cycles, so the request bound never drops a
range permanently.

A syncing repository enters ready only after its durable catch-up checkpoints
cover the bounded known union, no Recoverable Backlog remains, and the existing
five-minute stability window completes. An agreed permanent gap does not block
ready status, but it keeps replica convergence false and every affected query
partial.

After a declared permanent gap, the first segment after the missing range is
accepted as a new hash-chain head because the skipped segment hash is unknown;
the following segments must continue from that newly accepted hash as usual.

A Source writes every unacknowledged signed segment to its SQLite delivery
journal before attempting transfer, and removes it only after its Collector
acknowledges the continuous watermark. The journal uses Zstandard level 1 when
beneficial and is released incrementally after acknowledgement. Below the
existing 256 MiB filesystem safety guard, a Source enters explicit capture
suspension rather than creating a cursor, acknowledgement, or permanent gap.

## Query result

History responses include repository identity, observed and received coverage,
watermarks, gaps, clock skew, and one of `complete`, `partial`, or `local_only`.
Queries are bounded, paginated, and resolution-specific; arbitrary SQL and
unbounded export are forbidden.

Membership replacement accepts only existing cluster `node_ids`; XP derives each
node's pinned repository identity and initializes added members as `syncing`.
Administrators cannot write lifecycle, convergence, capacity or identity fields.
Every source node derives the same pinned identity from cluster material; an ordinary source may
send only its own identity, while a repository sender must exactly match its current Raft member
identity. Receivers reject a same-node-id segment with substituted public keys before accepting
or forwarding it. A history replay from an already-serving repository may cover a retired source
node, but only when its identity exactly matches the deterministic cluster-derived identity.

`subject_node_id` scopes records and coverage to one node for the existing
runtime, traffic, connection, and IP views. `page_size` and `page_cursor` remain
bounded by the repository query limit; a response supplies `next_page_cursor`
only when another bounded page is available.
Binary `record_key` and `payload` fields are unpadded base64url so actual JSON
response bytes remain within the query response budget.
