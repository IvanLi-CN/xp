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

Repository summary, repair, initial-backfill and anti-entropy direct requests use
the target node's registered public HTTPS `api_base_url` only. A configured Mesh
endpoint is not selected or probed, and direct requests never enter Reverse Mesh.
Source delivery uses the same public endpoint. When public transport fails, the
durable checkpoint or source outbox remains unchanged for the next bounded retry;
history synchronization never opens a Mesh relay. Legacy relay payloads remain
parseable at the receive boundary for wire compatibility, but the repository
worker never constructs or sends them.

- Ready local standbys use the strict receiver and durable ACK path; non-Ready fallback is excluded.
- A failed standby resets the primary failure count; the next cycle retries the registered primary.
- Three successful standby pages also restore primary selection.
- Failover never resets cursors, changes epochs, or rearms consumed recovery.

Initial-backfill responses are first capped at four times the 192 KiB semantic page budget on the
JSON wire body, and record-count overflow is rejected before typed record decoding. Pages are
then capped at 128 records and 192 KiB. Historical-source pages use their JSON record budget;
Ready-tiered pages use the sender's canonical segment-byte budget. Receivers enforce both limits
before decoding
records and validate the applicable opaque cursor family for length, format,
forward progress, and stable snapshot/export state before persisting the next
checkpoint. Malformed, regressing, or cycling cursors remain retryable failures.

The first repository baseline uses a separate signed backfill chain. Schema records use a stable
`<stream>-backfill-<source_node_id>` stream family; tombstones use the matching
`tombstone-backfill-<source_node_id>` stream and the backfill page's independent stream state.
Backfill does not consume or advance the live source outbox, live sequence head, or live hash-chain
state. If the live source epoch is unset, the first non-empty page may durably initialize that
identity epoch before creating its independent sequence chain; this is not a live sequence advance.

A predecessor-format page already queued in the live source journal keeps its original signed
stream. The worker retrieves missing predecessors from registered peers over signed public HTTPS,
verifies and durably imports each bounded chain page, and resumes after a restart. It accepts the
original pending page only after its chain is complete; the records, page checkpoint and genuine
collector ACK for its exact original IDs commit atomically. Missing predecessors are retried without
inventing a permanent gap. Subsequent newly collected backfill pages use the independent stream.

When an expired repository has a pending legacy page and no Ready peer, it scans registered sources
over signed public HTTPS before replay. The initial-backfill request uses `stable_deletions=true`;
the response contains only deletion markers and a `deletion_snapshot` bound to the cluster, source
identity, marker set and durable deletion version. Each continuation preserves that snapshot and the
fixed observation time. A changed snapshot restarts the bounded export.
A missing or invalid snapshot fails closed and remains retryable.
Deletion evidence and page progress commit atomically against the current source membership.
Later deletions use the normal durable source delivery path.
The source assigns each repeated deletion a durable generation and binds it to the signed tombstone
and acknowledgement. An older acknowledgement cannot discharge the newer generation. Remote cleanup
only completes the generation sent in that request; a failed write retains prior intent.
Deletion pages count the JSON array brackets and separators in the byte cap.

Before applying a persisted local page, the receiver reloads the exact signed wires and checks the
current cluster binding, derived repository identity, segment ownership, signature, source node,
backfill epoch, stream suffix, expected sequence, and predecessor hash chain. The page's computed
stream state must equal the checkpointed stream state. A mismatch, malformed wire, capacity failure,
or storage error fails closed and leaves the page checkpoint and its original wires available for
retry.

Wire acceptance, record/tombstone import, receiver state, backfill stream state, page cursor, and
completion are one durable mutation. A cursor is not advanced by a partially imported page.
New-format wire IDs remain in the durable in-flight page until the receive/apply ACK and checkpoint
commit succeeds; predecessor source-journal rows remain until a real collector ACK covers the exact
ID.
Only the matching durable boundary may remove them. Restart replay is exact and idempotent, so it
cannot allocate new live sequences or duplicate imported records.

Tiered exports use canonical retention-start keys; raw sample timestamps remain unchanged.

- Order and continuation: `(observed_start, source_node_id, source_epoch, stream, sequence)`.
- Storage, sender and receiver share one aggregate/uptime/resource retention-range decoder.
- New opaque cursors declare `keyset_version=2`; payload and source identities remain unchanged.
- Reject predecessor cursors with an `after` key before seeking; restart the bounded export.
- Restart uses existing idempotent imports and never rearms the recovery generation.
- An unstarted predecessor cursor can upgrade in place without changing its fixed export state.
- Unknown versions and version regressions fail closed; source delivery stays sequence-strict.

## Acknowledgement and repair

An acknowledgement advances only a continuous watermark. Expired cursors return
the earliest retained cursor and an explicit gap. Tombstones replicate before
affected records and remain until every current ready repository acknowledges
them plus the tombstone horizon. Anti-entropy exchanges partition summaries,
repairs ranges first, then drills down.

Prefix tombstones target matching subject, observer, schema and key prefix across sources and
epochs. Exact tombstones also cover the corresponding target stream and backfill stream. Replay
protection follows the same scope as SQLite deletion, and an atomic batch applies deletions after
record upserts so a record from the same batch cannot reappear.
Historical recovery and repair preserve the original verified wire, suppress only records covered by
known durable tombstones, and atomically persist the remaining records, deletion state and receipt.
Ordinary live delivery retains strict resurrection rejection.
Tombstone expiry is committed with the receiver checkpoint and records; a failed batch preserves
the prior durable tombstone and receiver state.
Expiry identifies the exact source, epoch, stream and sequence; newer tombstones with the same key
remain durable and continue to suppress matching records.

SQLite repositories maintain v3 summaries in the existing `history.sqlite3`.

- A block covers 4096 sequence positions; its canonical leaf excludes local `received_at`.
- `summary_version=3` explicitly selects this format.
- `admin.repository-history-summary-v3` is advertised only when metadata is ready.
- Missing, older, or unknown requested versions return the v1 partition path.
- Older peers' v2 responses use legacy partitions, never a v2/v3 sequence-digest comparison.
- SQLite adds `digest_version` with a v2 default; v3 digests have a one-byte format prefix.
- No historical row is rewritten. Old or unprefixed metadata is rebuilt before publication.
- The prefix also detects a predecessor rewriting digests without updating the new column.
- Predecessor binaries cannot consume prefixed metadata; rollback deep checks fail closed.
- Forward deployment repairs metadata in place, without an extra database.
- Mutation triggers mark affected blocks without resetting the durable migration cursor.
- A five-second blocking worker seeds at most 4096 keys or rebuilds at most 16 blocks.
- The final dirty-block page persists completion under the replica lock without an idle tick.
- Dirty metadata prevents publication until rebuilt; continuous writes invalidate affected blocks.

Deep verification compares retained metadata even when a permanent gap exists.

- Matching retained records and gap ledgers may complete the verification cycle.
- `history_truncated` and query `partial` persist; Raft `replica_converged` stays false.
- Missing, incomplete or unknown metadata cannot count as verified.
- Metadata rebuilding alone does not restart historical backfill.

An unconsumed recovery may refresh a repair page after its retained anchor expires.

- Keep still-retained pending IDs first; fill up to 64 IDs from the current summary page.
- Re-read the same cursor after draining; do not skip omitted current IDs.
- Gap merge and checkpoint/response-identity replacement commit atomically.
- Validate repair and refreshed-summary gap pages separately at the unchanged 64-gap limit.
- Merge both pages within that atomic refresh.
- An invalid second page rolls back the first merge and checkpoint.
- Preserve generation, its source binding and receiver watermark; reject stale refresh state.

The bounded repair response contains `segments`, `gaps`, and the additive
`unavailable_segment_ids` field. The latter lists only requested 64-character
hex segment IDs that the serving repository no longer retains under the
unchanged retention policy, such as when a summary/repair request crosses the
seven-day minute-tier boundary. It is limited to 64 IDs, must be unique, and
does not carry payload or acknowledgement meaning. A syncing peer may remove
only the exact IDs it requested and received in this field; duplicate, unknown,
malformed, or out-of-page IDs fail closed. Older responses that omit the field
are interpreted as an empty list, preserving wire compatibility.

A repair response may add `response_id`, a digest of its segments, unavailable
IDs, gaps, and truncation flag. The receiver derives the same digest when an
older peer omits it. An interrupted response with the same digest reuses its
stream allowance. If the serving repository explicitly rejects a stale digest,
or an older peer returns changed content, the receiver clears only the persisted
response identity and retries the same pending segment set; its cursor, gaps,
tombstones, and handoff state remain durable. A response whose supplied digest
does not match its content, contains unknown IDs, or does not advance the pending
set still fails closed.

For a truncated response, a newer peer may also include bounded
`retained_stream_boundaries` and a separate content digest. Each boundary is the
minimum retained `first_sequence` for one source/epoch/stream, computed from the
serving repository's durable segment rows. A consumed-generation continuation
without a pre-existing transient gap is accepted only when the first returned
segment equals that declared boundary and the content digest matches. The legacy
`response_id` remains unchanged so older peers continue their v1 fallback; absent
or changed boundary metadata remains fail-closed.

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

Repository summaries also carry an additive `history_truncated` marker when the
sender has observed more gap evidence than the bounded ledger can retain. Peers
must preserve that marker and keep affected queries partial; absence of the
field is interpreted as `false` for older summaries.

A syncing repository enters ready only after its durable catch-up checkpoints
cover the bounded known union, no Recoverable Backlog remains, and the existing
five-minute stability window completes. An agreed permanent gap does not block
ready status, but it keeps replica convergence false and every affected query
partial.

For field acceptance, `ready` alone is not completion evidence. The operator must observe the
bounded backfill checkpoint advance to its terminal page through exact-current-SHA test evidence or
an authorized local checkpoint diagnostic, together with zero recoverable pending work and a stable
repository status sample. The public status endpoint does not serialize the local backfill cursor;
status samples alone cannot prove that checkpoint. Any permanent gap, `history_truncated`, missing
peer runtime, or query quality of `partial`/`local_only` remains an explicit incomplete-history
result.

After a declared permanent gap, the first segment after the missing range is
accepted as a new hash-chain head because the skipped segment hash is unknown;
the following segments must continue from that newly accepted hash as usual. A
repair response carrying `history_truncated=true` is the only additional proof
that a retained tail may cross an existing local watermark during initial
backfill, once per affected source stream; a wire-bounded page may span multiple
responses or summary pages. The receiver records each crossed range as
`source_retention_expired` and persists the consumed `(source, epoch, stream)`
allowance. An earlier contiguous segment or completed response does not consume
another stream's allowance, while a second handoff for the same stream remains
rejected once its permanent gap and receiver watermark are durable. If an older
runtime persisted only the consumed-stream marker and lost the active handoff
before those completion records, the marker is recoverable history rather than completion evidence:
the next retry may recreate the same handoff and response identity. The legacy response-complete
bit is response-scoped historical state, not per-stream completion evidence, so it must not block
another stream from recovering its own handoff. A completed handoff range is retained in the peer
checkpoint so gap ledger rotation cannot reopen it. If a restart finds the active handoff marker
after its permanent gap and receiver watermark were already persisted, completion is idempotent:
XP clears the marker and records the completed range without advancing the watermark twice.
Ordinary source delivery and anti-entropy
continue to require exact sequence continuity.

A stale retained-anchor checkpoint may be advanced only by the signed local
`xp-ops xp history-repository-recover` command. Dry-run returns a fingerprint without writing;
apply requires `--yes` and the expected fingerprint. The armed recovery generation crosses one
retention boundary once, updates the existing database and control snapshot in place, and is
idempotent across retry. It never creates a database copy, changes quota, deletes source outbox
rows, or converts a permanent gap into complete history.

An exact persisted segment replay remains an idempotent ACK after the receiving
runtime restarts; the durable segment identity and watermark are the authority,
not the in-memory recent hash window. A different segment ID at the same cursor
continues to fail fork validation.

A Source writes every unacknowledged signed segment to its SQLite delivery
journal before attempting transfer, and removes it only after its Collector
acknowledges the continuous watermark. The journal uses Zstandard level 1 when
beneficial and is released incrementally after acknowledgement. Below the
existing 256 MiB filesystem safety guard, a Source enters explicit capture
suspension rather than creating a cursor, acknowledgement, or permanent gap.

The delivery status is additive and durable. It exposes `byte_counter_unit=canonical-v1` together
with cumulative generated, durable-ACK, and
repository-apply segment/byte counters, attempt/success counts, categorized failures, and the last
attempt/progress timestamps. Enqueue, ACK, response, gap, watermark and receiver checkpoint
updates commit their matching counters in the same SQLite transaction; retries and restarts do
not double count. A 300-second sample window computes each stage as `delta(counter) / 300` in
both segments and canonical bytes. Durable `peak_generated_segments` and
`peak_generated_bytes` retain the highest completed five-minute generated window in the current
counter epoch. Source-to-ACK and repository-to-apply must each reach 10 times that peak generated
rate before a delivery convergence claim is valid. A 5-second source tick may
run sixteen local bounded pages; local receiver commits use four segments with a 50ms yield under
the 1 MiB wire budget, while remote protocol pages remain capped at 256 segments and 1 MiB. It
does not enable unbounded concurrency
or alter sequence, retention, generation, gap, quota or public-HTTPS path rules.
Runtime status also returns the last completed or elapsed window for each source/stream/target
scope, so the 10x claim cannot be inferred from another stream's aggregate progress. Missing
window or scope fields on an older peer default safely and do not change v1 fallback behavior.

- Mixed-stream repair pages select the first handoff allowed by the durable checkpoint. A
  preceding unrelated gap cannot preempt the armed source/epoch/stream. Completion on another
  stream preserves the generation binding and its consumed state.

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

A refreshed first summary page persists an explicit revisit marker, so its absent cursor is not
interpreted as end-of-export after one bounded repair. An all-expired page with no missing current
IDs persists gaps and the serving page continuation atomically without consuming its generation.
