# CLI Contracts（#0022）

## `xp-ops deploy`

- 范围（Scope）: external
- 变更（Change）: Modify

### 用法（Usage）

```bash
xp-ops deploy \
  --node-name <name> \
  --access-host <host> \
  --cloudflare \
  --account-id <id> \
  --zone-id <id> \
  [--cloudflare-token <token> | --cloudflare-token-stdin] \
  [-y|--yes] \
  [--non-interactive] \
  [--dry-run]
```

说明：

- `--cloudflare-token <token>`：本计划新增；用于在本次运行中提供 Cloudflare API token（敏感信息，输出必须脱敏）。
- `--cloudflare-token-stdin`：本计划新增；从 stdin 读取 token（用于减少 shell history / `ps` 泄露风险）。

### 参数（Args / options）

（仅列出与本计划相关的参数与规范；其余参数沿用既有契约/实现）

- `--cloudflare-token <token>`: Cloudflare API token（敏感信息）
  - default: none
  - conflicts: `--cloudflare-token-stdin`
  - precedence: 当该参数存在时，优先级高于 `CLOUDFLARE_API_TOKEN` 与 `/etc/xp-ops/cloudflare_tunnel/api_token`
  - output: 不得在 stdout/stderr 打印 token 原文；preflight 可显示 “provided via flag”
  - post-success: 成功部署后应提示用户轮换/废弃该 token（不输出 token 原文）
- `--cloudflare-token-stdin`: 从 stdin 读取 Cloudflare API token
  - default: false
  - conflicts: `--cloudflare-token`
  - behavior: 读取 stdin 全部内容并 `trim()`；空值视为缺失
  - output: 不得在 stdout/stderr 打印 token 原文；preflight 可显示 “provided via stdin”

### token 来源与优先级（normative）

当启用 Cloudflare（`--cloudflare` 且未显式 `--no-cloudflare`）时，token 的解析顺序为：

1. `--cloudflare-token` / `--cloudflare-token-stdin`
2. 环境变量 `CLOUDFLARE_API_TOKEN`
3. 文件 `/etc/xp-ops/cloudflare_tunnel/api_token`

若以上均不可用：必须失败并返回可操作错误信息（明确指出三种可用来源）。

### 输出（Output）

- 默认输出为 human-readable。
- preflight config 不得包含 token 原文；允许显示 token 来源（flag/env/file/absent）。

### 退出码（Exit codes）

（沿用既有 deploy 退出码；本计划关注的失败类目至少包含 token missing）

- 非 0：token missing / 参数不足 / Cloudflare API error / filesystem error 等（以实现阶段冻结的错误码为准）
