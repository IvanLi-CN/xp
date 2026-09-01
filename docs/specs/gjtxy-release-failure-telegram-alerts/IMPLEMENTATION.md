# Release 失败 Telegram 告警实现

## Current Status

- Implementation: complete
- Lifecycle: active
- Delivery path: Oidrune reusable workflow

## Coverage / rollout summary

- `notify_failure` 保留 `release` `workflow_run` completed/main/failure 过滤和目标 SHA 解析。
- `smoke_test` 保留 `workflow_dispatch` 手动路径，并使用独立的 smoke 标题。
- 两个通知 job 都固定调用 `IvanLi-CN/oidrune/.github/workflows/notify.yml` 的
  `e48822f99c6402a753ed86557ea029754cbab20b`。
- caller 侧声明 `id-token: write`，省略 `gateway_url` 与 `oidc_audience`，不再传递旧 Telegram
  secret。
- failure 与 smoke summary 由 xp caller 完整组装，包含项目、状态、workflow、目标 SHA、run URL、
  标题和原有上下文；首行保持旧 notifier 的 `🚨 Release Failed` / `🧪 Smoke Test` 与
  owner/repository 格式，smoke 状态保持为 `smoke test`。

## Contract validation

- `.github/scripts/test-notify-release-failure.py` 断言固定 SHA、无旧 secret wiring、OIDC 权限、
  触发过滤、失败判定、手动 smoke 路径与 caller-owned summary 字段。
- `.github/workflows/ci.yml` 在 Rust job 中执行该 contract test。

## Operational boundary

- 本次迁移不执行真实 `workflow_dispatch` smoke notification；真实 Telegram 验证仍需由授权
  operator 在 GitHub Actions 中手动触发。
