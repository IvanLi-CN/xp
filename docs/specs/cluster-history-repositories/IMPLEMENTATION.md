# 集群长期历史数据仓库实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖和 rollout 事实。

## Current Status

- Implementation: Wave 4 replica runtime and bounded internal API are active
- Lifecycle: active
- Catalog note: Initiative #248; five serialized Tickets

## Coverage / rollout summary

- SQLite storage and JSON migration: provided by the prior Waves.
- Repository control plane and node identity: persisted with the Raft desired state.
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

## Remaining Gaps

- Repository membership administration and deployment rollout validation remain Wave 5 work.

## Related Changes

- Issue #248: https://github.com/IvanLi-CN/xp/issues/248

## References

- `./SPEC.md`
- `./HISTORY.md`
