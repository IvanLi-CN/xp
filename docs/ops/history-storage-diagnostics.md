# History storage stall diagnostics

XP keeps a small node-local diagnostic record for the SQLite operations that can explain a
repository status or synchronization stall. The record is intended for an operator investigating an
incident on a host-managed or container node. It is not an API, a health signal, or a replacement
for the history database.

## Record

The file is `${XP_DATA_DIR}/history.sqlite3.diagnostics.json`.

## Summary repair status

Ready lifecycle alone does not prove that source delivery or deep verification has finished.

- Compare pending segment count, last ACK time and oldest cursor across status samples.
- Ready local standbys accept continuous backlog; failed standbys retry the primary next cycle.
- Neither path permits another recovery apply or relaxed sequence/hash checks.
- Incomplete v3 metadata blocks deep verification even when migration has finished.
- The final dirty-block page must publish completion without requiring an idle tick.
- Accept deep checks only with `summary_version=3` and `partitions_included=true`.
- Permanent gaps still require `partial` and `replica_converged=false`.
- A repeated `HashChainMismatch` at the first segment after a retained source boundary indicates a
  stale gap page. The fixed path carries the already durable adjacent `source_retention_expired`
  gap once; do not re-apply a consumed generation, edit the gap ledger, delete the outbox, or relax
  hash checks while diagnosing it.

The read-only repository runtime status includes `summary_repairs`, keyed by peer node ID. Each
entry
exposes `pending_count`, `summary_cursor`, `pending_next_cursor`, `last_attempt_at`,
`last_progress_at`, `last_result`, `last_error_code`, `retry_after`, and `attempt_count`.
`last_result` distinguishes `progress`, `explicit_unavailable`, `response_changed`, `no_progress`,
`transport_error`, `storage_error`, and `in_progress`.

When `pending_count` remains non-zero, collect the status twice across one lifecycle interval. A
changing `last_attempt_at` with `transport_error` points to the registered public HTTPS path;
`response_changed` means the same pending page was reissued with a refreshed response identity;
`no_progress` leaves the pending IDs intact for retry; and `storage_error` requires preserving the
SQLite database, WAL and diagnostic file before any restart. Do not re-apply a consumed recovery
generation, delete source outbox rows, increase quota, create a second database, or run full
`VACUUM` as a diagnostic shortcut.

## Initial baseline backfill and US4 sampling

The first repository baseline is not ordinary live source delivery. Schema records use a separate
signed `<stream>-backfill-<source_node_id>` chain, while tombstones use the matching
`tombstone-backfill-<source_node_id>` stream and backfill page state. A local page persists its
exact signed wires, IDs, backfill epoch,
stream state, page cursor and completion flag before import. On restart, XP must recheck the current
cluster binding, derived repository identity, segment ownership, signature, source/epoch/stream,
sequence continuity and predecessor hash before applying any record. A failed check or storage write
leaves the
page pending; it must not advance the cursor or delete an original wire. New-format in-flight wires
clear only at the durable receive/apply ACK boundary; predecessor source-journal rows require a real
collector ACK for the exact segment.

If the repository is older than the tombstone freshness horizon and a local page is still pending,
XP blocks the rebuild and replay to preserve the signed page and avoid accepting expired deletion
evidence. The error reports a stale rebuild blocked by an unacknowledged page, or a required
tombstone-safe rebuild. There is currently
no supported automatic or operator repair for this combined condition. Preserve the database, WAL,
source journal and signed page; collect diagnostics for investigation rather than deleting the page,
forcing freshness, or marking catch-up complete.

New local pages persist their creation time independently of historical record timestamps.
An older pending page without that time also remains blocked, even if another receive later updates
the repository verification time: that update cannot establish the age of the pending page.

For a read-only US4 field sample, record the candidate SHA, node ID and `XP_DATA_DIR`, then capture
`GET /api/admin/history-repositories` with the already-authorized admin credential on every
configured
repository node at `t0` and again after at least five minutes. Preserve both JSON responses and the
collection times. If a member remains `syncing`, continue sampling at the normal lifecycle interval;
one sample cannot establish a completed baseline.

At each sample, check every configured member's `runtime`: healthy `storage_mode`, zero
`source_delivery.pending_segments`, zero `summary_repairs[*].pending_count`, no unexplained
`history_truncated`, and advancing verification/checkpoint evidence. Then run one bounded
known-window
history query and retain its `complete`/`partial`/`local_only`, coverage, watermark and gap fields.
`ready` is only a lifecycle state: a permanent gap may coexist with it, but the affected query must
remain `partial` and `replica_converged=false`. Missing runtime, unreachable members, pending work,
degraded storage, `history_truncated`, a non-zero unexplained gap, or a changed candidate SHA
fails the
complete-history claim.

The status endpoint does not expose the local backfill page cursor or completion marker, so it
cannot
by itself prove terminal backfill progress or that a process restart replayed an interrupted page.
Mark those parts of US4 as unverified unless the evidence includes the controlled exact-wire replay
check (or the corresponding current-SHA test evidence). Never infer completion from a stable record
count,
`ready` alone, one zero-pending observation, or a UI screenshot.

## Delivery throughput

The read-only repository status also exposes durable `delivery_metrics`. The counters are cumulative
and monotonic within `counter_epoch`: `generated_segments`/`generated_bytes` measure source enqueue,
`acknowledged_segments`/`acknowledged_bytes` measure durable source ACK, and
`applied_segments`/`applied_bytes` measure repository receiver commits. The status includes source
`byte_counter_unit=canonical-v1`; these byte counters exclude the signature, while wire bytes remain
the journal capacity and page-budget unit. The status includes source
attempts, successes, `transport_error`, `response_error`, `response_changed`,
`explicit_unavailable`, and `storage_error` counters plus `last_attempt_at` and `last_progress_at`.
Missing fields from an older peer are treated as zero and do not change v1 fallback behavior.

Capture two signed public-HTTPS status samples exactly 300 seconds apart. Compute each stage as the
counter delta divided by 300, in both segments/second and canonical bytes/second; compare against
the highest sustained five-minute generated delta, exposed as `peak_generated_segments` and
`peak_generated_bytes` in the current counter epoch. Both source-to-ACK and repository-to-apply
must be at least 10 times that generated rate before declaring convergence. Pending depth and ACK
timestamps are diagnostic only and cannot prove the 10x gate. A 5-second source tick is bounded to
sixteen local working pages. Local repository commits use four segments with a 50ms yield between
batches; each page remains below the 1 MiB wire limit and remote protocol pages retain the
256-segment/1 MiB contract. A higher cadence does not permit unbounded memory, concurrency, or
retry work.

Runtime status also returns `delivery_metrics.last_completed_window` after a durable counter event
closes a 300-second window, or an ephemeral window when the current baseline has elapsed. It
contains the window start/end, segment and byte deltas, milli-rates, stage-specific 10x booleans,
and aggregate `meets_ten_x`. `delivery_scopes` separates cumulative generated, ACK and apply
counters by source node, stream and registered target repository; each scope also exposes its
own completed or current 300-second window, so a busy stream or target cannot hide a starved one.
The status view is diagnostic; release acceptance still requires two signed samples and the
real-node five-minute observation.

The owner-dispatched release workflow also requires an owner-supplied resource attestation bound to
the exact target SHA and workload-log SHA. The attestation commit must have the target as its only
parent and contain only the attestation and workload log; readiness recomputes the log SHA before
publication and requires a GitHub-verified valid pinned repository-owner signature from the
owner-triggered workflow. The release retains the evidence files and commit identity as assets. The
attestation must record a 300-second window,
`MemoryMax=128M`, `MemorySwapMax=0`, and successful segment and canonical-byte 10x gates for both
delivery stages. Missing, malformed, or SHA-mismatched evidence fails closed before release
artifacts are published.
The attestation fields are `schema=xp-history-throughput-v2`, `target_sha`, `log_sha`,
`window_seconds=300`, `memory_limit_mib=128`, `swap_limit_mib=0`, the observed
`sample_window_seconds`, `generated_segments`, `generated_bytes`, `peak_generated_segments`,
`peak_generated_bytes`, `acknowledged_segments`, `acknowledged_bytes`, `applied_segments`, and
`applied_bytes`, plus four boolean gates: `source_ack_meets_ten_x_segments`,
`source_ack_meets_ten_x_bytes`, `repository_apply_meets_ten_x_segments`, and
`repository_apply_meets_ten_x_bytes`. Readiness compares every numeric field with the immutable
workload log and recomputes the four gates from the observed peak-generated counters.

The evidence log must explicitly pass both workloads:

- `xp_source_delivery_journal_memory_e2e`
- `xp_repository_summary_memory_e2e`

Missing, ignored or failed workloads block publication. Any failed test result also blocks it.

If a consumed-generation retry has already left an exact non-permanent sequence gap, the next
repair response may promote that same range to `source_retention_expired` when the serving peer
proves `history_truncated=true`; this is a continuation of the existing checkpoint and should
reduce `pending_count`. A different range must remain pending with a storage-boundary diagnostic;
operators must not infer or apply a second retention crossing from the marker alone.

When the receiver has no transient gap because the source retention window moved after a delayed
retry, a newer serving peer may include `retained_stream_boundaries` in the authenticated repair
response. The receiver accepts a consumed-generation continuation only when the first returned
segment equals the source-declared minimum retained sequence for that stream; the boundary is
bound by a separate content digest while the legacy `response_id` remains available to v1 peers.
Missing or changed boundary metadata remains fail-closed and leaves the pending IDs intact.

The repair worker only uses the peer's registered public HTTPS `api_base_url`. A response that
explicitly lists unavailable IDs may reduce `pending_count` and records `explicit_unavailable`; it
does not alter permanent gap, `partial`, or `replica_converged=false` semantics. A segment that
cannot fit the 256 KiB repair wire budget is reported as `storage_error` before any checkpoint or
ACK change. Legacy order repair also stops before the 1 MiB wire page budget and resumes from its
durable cursor; malformed legacy rows do not contribute wire bytes to canonical ACK metrics.

- The file is private (`0600`) and atomically replaced.
- The serialized state is capped at 16 KiB and retains only the current in-flight operation, the
  latest slow operation, the latest slow leaf SQL operation, the last operation interrupted across
  a process restart, and bounded completion counters.
- Each event has a stable `operation_id`, a static `caller_class`, a static SQL template or
  composite-operation description, wall-clock start and finish times, monotonic `elapsed_ms`, an
  outcome, and an optional result count.
- SQL templates retain placeholders such as `?1`; they never include bind values. History rows,
  payloads, tokens, request bodies, and URLs are not recorded. `last_slow_leaf_event` preserves the
  most specific SQL boundary when a composite operation finishes after it.

The instrumented boundaries are:

- repository runtime status and its metadata-only record and segment counts;
- tiered backfill page execution, export lease refresh/finish/session checks, received-at cutoff,
  and both watermark reads;
- retention expiry probes, compaction pages, export leases, and replacement/prune maintenance.
- source-delivery journal summary reads used by runtime status.

Runtime status reads exact record and segment counts from the singleton
`repository_history_counts` row. Insert/delete triggers update that row in the same transaction as
the history mutation, including tombstones. Existing databases initialize it once from the durable
tables at startup; the first upgraded start can take longer while SQLite counts the existing rows.
Subsequent status requests do not traverse either history table. The status calculation runs on a
blocking worker so a slow SQLite operation cannot occupy a Tokio request worker.

Repository history queries also run on blocking workers. Subject-specific coverage uses the
`repository_history_records_coverage` covering index; incomplete aggregate checks seek by subject
through `repository_history_records_aggregate_completeness`. Slow coverage and incomplete aggregate
queries appear as `repository_history.coverage` and `repository_history.incomplete_aggregate` in the
bounded diagnostic file. The first start after this index is added builds it from existing rows.

An operation records its in-flight description in memory before the blocking boundary and queues it
to one dedicated, coalescing writer. The writer is rate-limited to one atomic replacement per 100 ms
and runs outside SQLite/backend locks, so diagnostics do not add a synchronous fsync to each query.
On normal completion it writes the elapsed result. If the process is restarted while an operation is
in flight, the next startup moves that description to `last_interrupted_event`. A failed operation
is retained with its scope-exit outcome when the guard is unwound. The file is evidence, not a
transactional health signal; collect it before restart because an abrupt process or host failure may
leave the last queued update unwritten.

## Incident collection

Preserve the diagnostic file before restarting or upgrading the affected node. Read it from the
node's configured data directory with an operator-approved read-only method, then correlate
`started_at_unix_ms`, `finished_at_unix_ms`, and `elapsed_ms` with the node's `/api/health`,
resource monitoring data, and host I/O or PSI observations. The diagnostic event identifies the
SQLite boundary; the health and host records establish whether that operation coincided with service
unavailability or storage pressure.

For the generated host-managed assets, collect the diagnostic, health, and host I/O evidence before
restarting. The commands are read-only:

```sh
# OpenRC and systemd generated assets use /var/lib/xp/data.
sudo -n date -u
sudo -n cat /proc/pressure/io
sudo -n cat /proc/loadavg
sudo -n cat /var/lib/xp/data/history.sqlite3.diagnostics.json
sudo -n ls -l /var/lib/xp/data/resource_metrics.sqlite3*
curl --max-time 5 -fsS http://127.0.0.1:62416/api/health

# With the operator's already-authorized admin token, capture current and I/O history.
curl --max-time 5 -fsS \
  -H "Authorization: Bearer ${XP_ADMIN_TOKEN}" \
  http://127.0.0.1:62416/api/admin/nodes/<node-id>/resources
curl --max-time 5 -fsS \
  -H "Authorization: Bearer ${XP_ADMIN_TOKEN}" \
  'http://127.0.0.1:62416/api/admin/nodes/<node-id>/resources/history?metric=cpu_iowait_percent'\
  '&limit=1500'

# Docker Compose uses the mounted path inside the official container.
docker compose exec -T xp cat /proc/pressure/io
docker compose exec -T xp cat /var/lib/xp/data/history.sqlite3.diagnostics.json
docker compose exec -T xp ls -l /var/lib/xp/data/resource_metrics.sqlite3*
```

Use the node's actual configured `XP_DATA_DIR` when it differs from the generated default; do not
substitute a guessed data directory. Also preserve the matching `history.sqlite3-wal` and
`history.sqlite3-shm` files, plus the `resource_metrics.sqlite3-wal` and
`resource_metrics.sqlite3-shm` files during collection. If the container is stopped, use its
read-only mounted volume path instead of `docker compose exec`:

```sh
docker compose run --rm --no-deps \
  --entrypoint ls xp -l /var/lib/xp/data
```

Do not start XP solely to collect evidence.

Do not delete `history.sqlite3`, its WAL, or the diagnostic file while collecting evidence. The
diagnostic file is safe to omit from a normal incident report if it contains no in-flight,
interrupted, or slow event; that is an evidence gap, not proof that SQLite was healthy.

## Acceptance checks

The implementation must pass these focused checks before publication:

```text
cargo fmt -- --check
cargo test --lib history_storage
cargo test --lib history_repository::replica::runtime
```

The tests verify the bounded JSON shape, private-operation redaction, restart recovery, the real
record-count path, and the existing history repository synchronization and retention semantics.
The instrumentation does not alter API responses, Raft state, history rows, cursors, leases,
retention decisions, or synchronization behavior.
