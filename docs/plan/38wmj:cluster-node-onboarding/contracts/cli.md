# 命令行（CLI）

## `xp-ops deploy`（join mode）

- 范围（Scope）: internal
- 变更（Change）: Modify

### 用法（Usage）

```text
xp-ops deploy \
  --join-token <TOKEN> | --join-token-stdin \
  --node-name <NAME> \
  --access-host <HOST> \
  [--api-base-url <HTTPS_ORIGIN>] \
  [--cloudflare | --no-cloudflare] \
  [--account-id <ID>] \
  [--zone-id <ID>] \
  [--hostname <FQDN>] \
  [--tunnel-name <NAME>] \
  [--origin-url <URL>] \
  [--xray-version <SEMVER|latest>] \
  [--enable-services | --no-enable-services] \
  [-y] \
  [--non-interactive] \
  [--overwrite-existing] \
  [--dry-run]
```

### 参数（Args / options）

- `--join-token <TOKEN>`: join token（推荐仅用于手工；有泄漏风险）
- `--join-token-stdin`: 从 stdin 读取 join token（推荐）
- `--overwrite-existing`: 允许覆写已有的 `/etc/xp/xp.env`（仅当 token 不一致或需修正时）
- 其余参数与 `xp-ops deploy` 同源（Cloudflare/enable/非交互/预演等），以实现阶段为准

### 行为（Behavior, normative）

- `xp-ops deploy` join 模式 MUST 串联以下步骤（与现有 deploy 同一套实现复用）：
  1. 安装依赖（`xray`；`cloudflared` 取决于 `--cloudflare`）
  2. 初始化目录/用户/服务文件（同 `xp-ops init`）
  3. 安装 `xp`（来源/策略沿用现有）
  4. 执行 join（不得执行 `xp init`）并获取集群 `XP_ADMIN_TOKEN_HASH`（见 `contracts/http-apis.md`）
  5. 写入 `/etc/xp/xp.env`（必须包含从 leader 同步到的 `XP_ADMIN_TOKEN_HASH`；不得写入明文 token）
     6.（可选）启用并启动服务（同 `--enable-services`）
- join 模式不得要求用户额外输入 admin token；token 获取必须在“只提供 join token”的条件下完成。
- 当 `/etc/xp/xp.env` 已存在且 `XP_ADMIN_TOKEN_HASH` 与 leader 下发值不一致：
  - 默认 MUST 失败
  - 仅当 `--overwrite-existing` 才允许覆写
- 日志与输出不得包含明文 token；错误信息可提示“从 stdin 输入/从 Web 复制”的操作建议。

### 输出（Output）

- Format: human（v1）；若后续需要 Web 集成，可在后续计划增加 `--json`（不属于本计划范围）

### 退出码（Exit codes）

- `0`: 成功
- `2`: invalid_args / missing_required_input
- `3`: preflight_failed / unsupported_platform
- `4`: filesystem_error / permission_denied
- `5`: join_failed（`xp join` 失败或返回非 0）
- `6`: token_mismatch（检测到现有 token 与输入不一致且未允许覆写）

### 兼容性与迁移（Compatibility / migration）

- 与现有 `xp-ops deploy` 共存；实现阶段需明确“推荐路径”并在 Web 引导中只展示推荐命令。

## `xp-ops deploy`（admin token 注入）

- 范围（Scope）: internal
- 变更（Change）: Modify

### 变更点（Change）

- join 模式下：`xp-ops deploy` 写入 `/etc/xp/xp.env` 时 MUST 使用从 leader 同步得到的 token（不得生成随机 token）。
  - 非 join 模式：保持现有行为（无 `/etc/xp/xp.env` 时可生成随机 token），但 Web 引导的“多节点场景”不得依赖该路径。

### Bootstrap token（print once）

- 在“bootstrap 节点首次部署”（非 join 模式，且 `/etc/xp/xp.env` 不存在）时：
  - MUST 生成随机明文 admin token（仅驻留内存用于打印）
  - MUST 写入 `/etc/xp/xp.env` 中的 `XP_ADMIN_TOKEN_HASH`（不得写入明文）
  - MUST 将明文 token **仅打印一次** 到 stdout（供管理员保存并用于 Web 登录）
  - MUST NOT 在日志或后续输出中再次回显该 token
