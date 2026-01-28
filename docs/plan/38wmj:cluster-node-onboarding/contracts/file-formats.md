# File formats

## `/etc/xp/xp.env`

Scope: internal\
Change: Modify

### Purpose

`xp` 服务的运行配置文件（env file）。本计划要求：服务端不再保存 `XP_ADMIN_TOKEN` 明文，仅保存其 hash，并在 join 时同步 hash 以保证集群一致。

### Format

- UTF-8 text
- Line-based `KEY=VALUE`
- Blank lines and `#` comments ignored

### Keys (normative)

- `XP_ADMIN_TOKEN_HASH` (required): admin token hash used to verify `Authorization: Bearer <token>` for `/api/admin/*`
  - Value format (v1): PHC string for Argon2id, e.g. `$argon2id$v=19$m=65536,t=3,p=1$...`
  - Recommended params (v1, normative defaults):
    - `m=65536` (64 MiB)
    - `t=3`
    - `p=1`
  - Token plaintext is expected to be high-entropy (bootstrap: randomly generated), so the hash is primarily to avoid persisting plaintext.
  - Verification: server MUST verify using Argon2id against the stored PHC string (salt and params are encoded in the PHC string).
- `XP_ADMIN_TOKEN` (deprecated): MUST NOT be required after this plan ships.
  - Migration behavior to be defined in implementation: if present, server MAY compute `XP_ADMIN_TOKEN_HASH` and ignore plaintext for verification; plaintext should not be persisted back.

### Compatibility rules

- New builds MUST accept `XP_ADMIN_TOKEN_HASH` in PHC Argon2id form.
- During migration window, builds MAY also accept legacy `XP_ADMIN_TOKEN_HASH` in `sha256:<hex>` form (if encountered in existing installs).
- During migration window, builds MAY accept `XP_ADMIN_TOKEN` but MUST treat it as deprecated and MUST NOT rely on it for join-time distribution.

### Security

- File permissions MUST be `0640` (or stricter) and owned by `root:xp` (or distro-equivalent), consistent with current repo conventions.
- No tooling (xp / xp-ops) may print the plaintext admin token during normal operation.
