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
standby at low frequency. Only when both direct paths fail may the source
attempt a jittered hourly relay through an eligible Mesh member. Relay is
streaming only, carries end-to-end X25519 plus AEAD payloads, and persists no history.

## Acknowledgement and repair

An acknowledgement advances only a continuous watermark. Expired cursors return
the earliest retained cursor and an explicit gap. Tombstones replicate before
affected records and remain until every current ready repository acknowledges
them plus the tombstone horizon. Anti-entropy exchanges partition summaries,
repairs ranges first, then drills down.

## Query result

History responses include repository identity, observed and received coverage,
watermarks, gaps, clock skew, and one of `complete`, `partial`, or `local_only`.
Queries are bounded, paginated, and resolution-specific; arbitrary SQL and
unbounded export are forbidden.
