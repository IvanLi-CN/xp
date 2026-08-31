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
  than by content hash. Startup repairs the corresponding SQLite cursor index in place for
  existing segments before they are served for repair; it preserves each signed payload and does
  not rebuild the database or run a full `VACUUM`.
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
  cursor when the existing 256 MiB filesystem guard is reached.
  `path_health.v1` reads a bounded telemetry source view directly from runtime state: rotating
  through at most 16 peers, with each peer's latest one-minute bucket, rather than cloning complete
  local 24-hour telemetry series. It bounds copied strings and latency samples before adding each
  peer only when the serialized source view still fits the 32 KiB source-record budget.
  After three failed primary delivery cycles, a source selects its rendezvous
  standby; both collectors accept the signed segment so that the transition has no coordination
  race. When both direct paths fail, an hourly-jittered relay carries compressed encrypted,
  frame-budgeted pending-source pages through an eligible cluster member without storing history
  at the relay.
  The SQLite source delivery journal maintains transactionally updated pending-count, pending-byte
  and epoch high-water statistics plus the last successful acknowledgement path/time.
  A delivery-order expression index serves tombstone-priority pages without a temporary sort.
  Restart hydration reads at most 256 rows and the persisted epoch high-water instead of decoding
  the entire journal.
  Existing databases initialize these fields idempotently without deleting or rewriting signed
  pending segments.
  Receivers require the complete pinned identity: repository senders must match their current
  Raft member identity, while ordinary cluster-node sources use the same server-derived pinned
  identity. Repair and relay batches apply the identical identity check before signature
  verification.
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
