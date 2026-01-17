# CLI

## xp config flags（rename）

- 范围（Scope）: internal
- 变更（Change）: Modify

### 变更点（Change）

- `--public-domain <DOMAIN>` → `--access-host <HOST>`
- 语义：订阅/客户端连接使用的 host（允许域名或 IP）。

### 约束

- 不新增新的 env 变量（保持现状：仅 CLI flag）。
- 默认值与校验规则保持与旧字段一致（空值允许，但订阅会报错）。

### 兼容性与迁移（Compatibility / migration）

- 破坏性变更：旧 flag `--public-domain` 移除。
