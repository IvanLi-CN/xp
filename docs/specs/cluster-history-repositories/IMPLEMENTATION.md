# 集群长期历史数据仓库实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖和 rollout 事实。

## Current Status

- Implementation: Repository runtime, administration and rollout integration are active
- Lifecycle: active
- Catalog note: Initiative #248

## Coverage / rollout summary

- SQLite storage and JSON migration: provided by the prior Waves.
- Repository control plane and node identity: persisted with the Raft desired state.
- Repository administration and observability: `PUT` / `GET /api/admin/history-repositories`
  validates existing node IDs and replaces Raft-backed membership, deriving pinned identities and
  preserving worker-owned lifecycle, convergence and capacity. It reports lifecycle, capacity,
  SQLite mode, freshness and partial/unreachable status. Repository query responses retain bounded
  pagination and return node-scoped coverage, watermarks, gaps, skew and completeness to the
  existing node runtime, traffic, connection and IP views. Syncing members perform bounded repair
  catch-up and enter `ready` only after five stable minutes; successful deep verification writes
  local convergence back to Raft.
- Initial repository bootstrap is deliberately yieldable: each worker tick exports at most one
  local page and one bounded page per peer (128 records / 192 KiB). Before any member is `ready`,
  a syncing repository imports each peer's node-local history, including another configured
  syncing repository. This establishes the complete common baseline without recursively starting
  repository repair. The existing opaque page cursor fixes the initial snapshot's upper time
  bound, so samples created during the transfer do not extend that bootstrap scan. `InProgress`
  keeps the member in `syncing` without writing `CatchUpIncomplete`; capacity refresh, live source
  collection and lifecycle checks continue on their normal cadence. A repository enters the
  existing five-minute stability window only after every page is complete. A local page records
  its pending wire set before delivery and commits all acknowledgements with the page cursor; an
  interrupted tick replays those unchanged wires instead of assigning new source sequences. For a
  ready peer, the single page budget is spent on one summary page, one repair response, or one
  tiered export page; the summary cursor and pending repair IDs are part of the durable peer
  checkpoint, so a restart resumes the same page instead of restarting an unbounded scan. Deep
  partition mismatches after a segment repair drains mark the checkpoint for the single-authority
  tiered import. A fresh summary verification pass must complete before the member can enter the
  readiness window.
- Tombstones received while a repository is `syncing` are atomically stored with their local
  cursor and acknowledgement page, but acknowledgement fanout is deferred until the Raft
  membership reports `ready`. The durable page is then retried through the existing all-node
  acknowledgement path, and its delivery cursor advances only after successful fanout.
- Startup lifecycle ticks repair the legacy mutable tombstone metadata where `created_at=0` and
  `expires_at` is the fixed horizon. The repair extends the ledger from the current Unix time and
  persists only the control snapshot; signed segments, cursors, hashes, ready membership and
  acknowledgement state are left unchanged. Operators must not delete `history.sqlite3`, replace
  repository members or change ordinary-node retention settings during rollout.
- Repository segment pages are ordered by tombstone phase and the signed source cursor, rather
  than by content hash. Existing databases persist `order_repair_cursor_id` and
  `order_repair_completed` in the source journal state row. A source collection cycle repairs at
  most 256 rows by primary-key order, decoding `wire` only for rows whose source metadata is still
  empty; the cursor, metadata and epoch high-water are committed together. A failed page leaves
  the previous cursor intact and the next process resumes from it. Ordinary status, epoch and page
  reads never start repair, and no partial index is created because its initial build would scan
  the complete journal. While repair is incomplete, new signed rows are persisted but are not
  offered to a collector; status reports `journal_order_repairing` with the durable backlog count.
  The repair preserves each signed payload and does not rebuild the database or run a full
  `VACUUM`.
- Repository summary pages use a metadata-only SQLite projection of `id` and tombstone phase.
  They do not load or deserialize segment payloads merely to enumerate IDs or advance the
  continuation cursor. Repair and backfill retain the separate full-payload path, so existing
  signed rows and the summary wire shape remain unchanged. When a peer is stuck in `syncing`
  because its summary request times out, upgrade the serving repository first and let the next
  five-minute direct-path retry resume the persisted catch-up; do not restart the source or
  delete its backlog as a recovery shortcut.
- When a serving repository confirms an expired permanent sequence gap, the receiver advances
  its cursor without inventing the skipped segment hash. The first segment after that range
  establishes a new hash-chain head, and subsequent segments resume ordinary continuity checks.
- A repair page may race the seven-day minute-tier segment cache: a segment advertised by summary
  can be pruned before repair reads its payload. The serving repository returns those requested IDs
  in `unavailable_segment_ids`; the syncing peer removes only that explicit set from its bounded
  checkpoint and continues at the saved cursor. This preserves the source outbox and all locally
  durable history while preventing an expired repair page from pinning the member in `syncing`.
- A ready peer's retained segment cache can also begin a source stream at a nonzero sequence when
  its predecessor has already expired. The initial summary-repair path accepts that signed frame
  as an unanchored retained tail and keeps `hash_chain_verified=false`; it then requires strict
  contiguous sequence/hash links for the remaining page. Ordinary source and anti-entropy receipt
  keeps rejecting the same frame, so the relaxed boundary cannot bypass live fork protection.
- Deep-verification partition summaries are persisted in the replica control snapshot and rebuilt
  from SQLite in bounded keyset pages. Until the rebuild reaches the end of the row set, a summary
  returns segment and gap metadata with `partitions_included=false`; this keeps catch-up available
  without making an HTTP request deserialize the retained payload window. The worker does not mark
  daily deep verification successful for such a response. Ordered appends update a completed cache;
  late rows, tombstone deletion, and retention replacement reset it for another bounded rebuild.
  A malformed row defers cache progress while allowing ordinary segment/gap replication to continue.
  If startup cannot create the additive summary keyset index for an external-history database,
  XP preserves its durable rows, exposes history storage as unavailable, and rejects history reads
  and writes rather than selecting a potentially stale JSON fallback.
- Incremental sync transport and path selection: accepted signed segment state is restored from the
  repository SQLite boundary. Every peer tracks direct Reality Mesh and Cloudflare Tunnel health,
  keeps a stable path with hysteresis, and probes the standby path at low frequency before source
  or repository work may use its Raft-assigned Reality Mesh Reverse route, then its independently
  paced dynamic relay.
- Every node produces bounded one-minute signed source segments for runtime, traffic, Mesh path
  health, inbound-IP and connection summaries. Each schema family has its own durable outbox,
  cursor, sequence and hash chain; pending segments retry unchanged until the rendezvous primary
  acknowledges them. Every unacknowledged segment is also persisted in the SQLite source delivery
  journal; the journal is replayed oldest-first after restart and released incrementally only after
  a continuous acknowledgement. Transport failures and legacy queue pressure remain recoverable
  backlog, never a permanent gap. The source enters `source_storage_guard` rather than advancing a
  cursor when the existing 256 MiB filesystem guard is reached. The SQLite outbox also enforces a
  fixed 128 MiB or 20,000-segment cap with a durable `capacity_suspended` marker: at 80% either
  dimension it reports `journal_capacity_guard`, rejects new source rows before the transaction
  writes, and resumes only after both dimensions fall below 60%; no unacknowledged row is deleted.
  Backpressure ranges retained in snapshots from the pre-journal queue implementation remain
  recoverable and are replayed through the persisted gap-page cursor; the current SQLite capacity
  guard rejects capture before allocating a source cursor and therefore creates no new such range.
  `path_health.v1` reads a bounded telemetry source view directly from runtime state: rotating
  through at most 16 peers, with each peer's latest one-minute bucket, rather than cloning complete
  local 24-hour telemetry series. It bounds copied strings and latency samples before adding each
  peer only when the serialized source view still fits the 32 KiB source-record budget.
  After three failed primary delivery cycles, a source selects its rendezvous
  standby; both collectors accept the signed segment so that the transition has no coordination
  race. When both direct paths fail, an hourly-jittered relay carries compressed encrypted,
  frame-budgeted pending-source pages through an eligible cluster member without storing history
  at the relay.
  A target returns the signed source-delivery receipt once the segment is durable. Tombstone
  acknowledgement fanout to other repositories is best-effort and logged for retry; a transient
  fanout failure never converts an already persisted source delivery into a 5xx response.
  The SQLite source delivery journal maintains transactionally updated pending-count, pending-byte
  and epoch high-water statistics plus the last successful acknowledgement path/time. The
  order-repair cursor and completion marker are initialized idempotently in the schema transaction
  without decoding payloads; a journal with no legacy rows is marked complete in constant time
  after the initialization aggregate. Replay pages contain at most 256 segments and 1 MiB of wire
  data, so a large backlog cannot inflate the XP process working set. A capacity-suspended source
  skips only new capture and continues replaying existing rows; each source worker cycle processes
  at most four successful replay pages and stops immediately on a failed page.
  The follow-up hk2 canary must observe ten consecutive 60-second source cycles with CPU at or
  below the 10% node quota, bounded journal reads, and no loss of Direct/Public or control-plane
  health. The shared resource test measures journal CPU/read/RSS bounds in isolation; the canary
  is the evidence for listener and control-plane availability. Any failed window stops rollout
  and preserves the journal for rollback.
  The additive `repository_history_segments_sync_order_v2` index serves tombstone-priority
  summary pages without a temporary sort. Continuation first resolves its opaque ID to the five
  persistent ordering columns, then binds a row-value range directly; it does not use a nullable
  cursor predicate or a scalar cursor subquery that prevents the SQLite planner from seeking.
  Existing databases create the v2 index idempotently without deleting the legacy index or
  rewriting signed segment payloads. If that startup creation fails for an existing external
  history database, the process retains SQLite and propagates the storage error instead of
  selecting the JSON fallback.
  Restart hydration reads at most 256 rows and the persisted epoch high-water instead of decoding
  the entire journal.
  The summary memory regression uses the shared testbox's summary-only mode to start the release
  `xp run` binary with 257 near-limit SQLite segments, call the signed summary endpoint repeatedly,
  and sample `smaps_rollup` under the 128 MiB/no-swap cgroup without a concurrent peer workload.
  Existing databases initialize these fields idempotently without deleting or rewriting signed
  pending segments.
  Live receivers require the complete pinned identity: repository senders must match their current
  Raft member identity, while ordinary cluster-node sources use the same server-derived pinned
  identity. History replay from an already-serving repository also accepts a retired source node
  when its identity exactly matches the deterministic cluster-derived identity; ready-repository
  repair/relay batches use that replay check, while ordinary source relay batches still reject
  substituted public keys before signature verification.
- Replica, retention and query selection: ready repositories run bounded five-minute repair and
  daily deep verification scheduling, preserve gaps/forks/unknown schemas/tombstones across
  restart, retain source segment repair state, transform older repository history into aggregates,
  anonymize IP identifiers after seven days, and select the healthiest most complete ready response.
- The proxy configuration, proxy client, proxy listener, proxy status, and compatibility path were
  removed. The dynamic relay contract remains separate from peer-direct transport and does not
  persist relay frames.
- Deployment parity: systemd, OpenRC and the single-image container keep the same persistent
  `${XP_DATA_DIR}/history.sqlite3` replica database. SQLite performs bounded incremental release;
  low disk or quota stops only history writes, and normal node-data retention behavior is unchanged.

## Remaining Gaps

- Aggregate acceptance must bind the final integration SHA after all serialized Wave PRs land.

## Related Changes

- Issue #248: https://github.com/IvanLi-CN/xp/issues/248

## References

- `./SPEC.md`
- `./HISTORY.md`
