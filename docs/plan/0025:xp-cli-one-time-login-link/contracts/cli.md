# 命令行（CLI）

## `xp login-link`

- 范围（Scope）: external
- 变更（Change）: New

### 用法（Usage）

```text
xp [global options] login-link [options]
```

说明：`[global options]` 复用 `xp` 现有全局参数（例如 `--data-dir`、`--api-base-url`、`--admin-token`）。

### 参数（Args / options）

None

### 输出（Output）

- stdout: `<login_url>\n`
- `login_url` 形如：`<api_base_url>/login?login_token=<TOKEN>`
- `login_token` TTL 固定为 1 小时（`3600` 秒）。

### 退出码（Exit codes）

- `0`: 成功
- `2`: 参数无效（例如 ttl 超出允许范围）
- `3`: 环境/文件缺失（例如缺少 cluster metadata、admin token 为空等）
- `1`: 其他错误

### 兼容性与迁移（Compatibility / migration）

- 不改变 `xp run|init|join` 的语义与默认行为。
- `login_token` 仅用于短期登录；长期访问仍建议使用 `admin_token`（或后续引入更完善的权限模型时再调整）。
