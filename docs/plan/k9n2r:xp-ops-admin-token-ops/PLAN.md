# xp-ops：admin token 设置后操作指引（#k9n2r）

## 状态

- Status: 待实现
- Created: 2026-01-29
- Last: 2026-01-29

## 背景 / 问题陈述

- 管理员在执行 `xp-ops admin-token set` 后，常见误解是“立刻生效”，但实际需要服务重启后新环境变量才会被读取。
- 集群多实例部署时，admin token 的校验必须在所有实例一致；缺少明确的“其它实例同步命令”会导致登录失败或行为不一致。
- OpenRC / shell `source` 场景下，argon2 PHC hash 形如 `$argon2id$...`，如果 env 文件写入时未做 shell-safe 处理，可能被 `$...` 变量展开破坏，导致服务永远鉴权失败（401）。

## 目标 / 非目标

### Goals

- `xp-ops` 提供统一的 `xp` 重启入口：`xp-ops xp restart`（屏蔽不同 init system 的差异）。
- `xp-ops admin-token set` 在完成写入 `XP_ADMIN_TOKEN_HASH` 后，输出可直接复制执行的下一步建议：
  - 当前节点重启命令
  - 其它实例同步相同 hash 的命令
  - 一个不泄漏明文 token 的验证示例
- 提供 `--quiet` 以便脚本化使用（仅输出机器可读的成功结果）。

### Non-goals

- 不在命令输出中打印或生成明文 token（避免 secret 泄漏风险）。
- 不实现“自动重启服务”（由运维显式执行，降低意外中断风险）。

## 需求（Requirements）

### MUST

- 新增 `xp-ops xp restart`：
  - 默认重启 `xp` 服务（可选参数覆盖服务名）
  - 支持 `--dry-run`（只打印将执行的重启动作）
- `xp-ops admin-token set`：
  - 能写入/更新 `XP_ADMIN_TOKEN_HASH` 到环境文件（不存在时创建）
  - 写入的 `XP_ADMIN_TOKEN_HASH` 必须可被 POSIX `sh` 安全 `source`（例如使用单引号包裹），避免 `$...` 展开破坏 argon2 PHC 字符串
  - 默认在 stderr 打印下一步命令建议（stdout 保持简洁、便于脚本判断）

### SHOULD

- 建议输出应可直接复制执行，并覆盖“当前实例 + 其它实例”的完整操作路径。

## 验收标准（Acceptance Criteria）

- When 运行 `xp-ops admin-token set --token <token>` 或 `--hash <phc>`，
  Then 命令成功后会打印 `xp-ops xp restart` 的建议与“其它实例同步命令”示例。
- When `xp-ops admin-token set` 写入了 `/etc/xp/xp.env`，
  Then `XP_ADMIN_TOKEN_HASH` 行可以被 `sh` `source`，且读取到的值仍是以 `$argon2id$` 开头的完整 argon2 PHC 字符串。
- When 运行 `xp-ops xp restart`，
  Then 会尝试通过系统 init system 重启服务，并对缺失/不支持的环境给出清晰错误。
- When 运行 `xp-ops admin-token set --quiet ...`，
  Then 不输出操作建议（只保留机器可读的成功结果）。

## 质量门槛（Quality Gates）

- Rust: `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings`
