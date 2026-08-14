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
  validates and replaces Raft-backed membership, then reports lifecycle, capacity, SQLite mode,
  freshness and partial/unreachable status. Repository query responses retain bounded pagination
  and return coverage, watermarks, gaps, skew and completeness to the existing node views.
- Incremental sync transport and path selection: accepted signed segment state is restored from the
  repository SQLite boundary; worker synchronization uses only peer-direct Reality Mesh and
  Cloudflare Tunnel paths.
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
