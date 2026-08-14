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
- Incremental sync transport and path selection: accepted signed segment state is restored from the
  repository SQLite boundary. Every peer tracks direct Reality Mesh and Cloudflare Tunnel health,
  keeps a stable path with hysteresis, and probes the standby path at low frequency before either
  source or repository work can use its independently paced dynamic relay.
- Every node produces bounded one-minute signed source segments for runtime, traffic, Mesh path
  health, inbound-IP and connection summaries. Each schema family has its own durable outbox,
  cursor, sequence and hash chain; pending segments retry unchanged until the rendezvous primary
  acknowledges them. Each bounded queue records stream-local backpressure as a durable permanent
  gap while returning its existing front for delivery, so one full stream cannot block another and
  recovery drains without reporting lost observations as complete.
  After three failed primary delivery cycles, a source selects its rendezvous
  standby; both collectors accept the signed segment so that the transition has no coordination
  race. When both direct paths fail, an hourly-jittered relay carries compressed encrypted,
  frame-budgeted pending-source pages through an eligible cluster member without storing history
  at the relay.
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
