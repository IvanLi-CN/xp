# File formats

## Persisted State (`XP_DATA_DIR/state.json`)

- 范围（Scope）: internal
- 变更（Change）: Modify

### Schema

- `nodes[*].public_domain` → `nodes[*].access_host`
- `schema_version`: 1 → 2

### 迁移（Migration）

- 当读取到 `schema_version=1` 且 `public_domain` 存在时：
  - 将值迁移到 `access_host`
  - 删除 `public_domain`
  - 写回 `schema_version=2`

## Cluster Metadata (`XP_DATA_DIR/cluster/metadata.json`)

- 范围（Scope）: internal
- 变更（Change）: Modify

### Schema

- `public_domain` → `access_host`
- `schema_version`: 1 → 2

### 迁移（Migration）

- 当读取到旧字段 `public_domain` 时：
  - 将值迁移到 `access_host`
  - 删除 `public_domain`
  - 写回新 `schema_version`

## Raft Snapshot (`XP_DATA_DIR/raft/snapshots/current_snapshot.json`)

- 范围（Scope）: internal
- 变更（Change）: Modify

### Schema

- Snapshot 内嵌的 `PersistedState` 采用与 `state.json` 相同的字段名（`access_host`）。

### 迁移（Migration）

- 与 `state.json` 相同的迁移策略；旧快照读取后应写回新格式。
