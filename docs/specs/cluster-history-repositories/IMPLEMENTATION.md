# 集群长期历史数据仓库实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖和 rollout 事实。

## Current Status

- Implementation: bootstrap contract created; runtime work in progress
- Lifecycle: active
- Catalog note: Initiative #248; five serialized Tickets

## Coverage / rollout summary

- SQLite storage and JSON migration: planned for Wave 1.
- Repository control plane and node identity: planned for Wave 2.
- Incremental sync transport and path selection: planned for Wave 3.
- Replica, retention and query selection: planned for Wave 4.
- Admin/Web integration and deployment validation: planned for Wave 5.

## Remaining Gaps

- Runtime modules, migrations, Protobuf definitions, HTTP routes, Web views,
  and deployment tests are not yet implemented.

## Related Changes

- Issue #248: https://github.com/IvanLi-CN/xp/issues/248

## References

- `./SPEC.md`
- `./HISTORY.md`
