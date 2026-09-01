# gjtxy · Release 失败 Telegram 告警接入

## Related ADRs

- None. This migration changes the notifier provider contract without introducing a new
  repository architecture or boundary decision.

## Summary

- 为 `release` 工作流补一个 repo-local notifier wrapper，统一复用固定版本的 Oidrune workflow。
- 为 release 目标 SHA 增加显式日志标记，确保失败告警能定位真实 release target。
- 保留 `workflow_dispatch` smoke test 路径，由调用方完整生成通知 summary。

## Scope

- 新增 `.github/workflows/notify-release-failure.yml`。
- 更新 `.github/workflows/release.yml` 输出 `RELEASE_REQUESTED_SHA` / `RELEASE_TARGET_SHA` 标记。
- `.github/workflows/notify-release-failure.yml` 的两个通知 job 固定调用
  `IvanLi-CN/oidrune/.github/workflows/notify.yml` 的已核验 commit SHA，省略 gateway
  与 OIDC audience 覆盖并声明 `id-token: write`。
- 通知调用方必须在 `summary` 中显式提供项目名、状态、目标 SHA、run URL 与失败/Smoke 标题，
  不依赖 Oidrune 自动补元数据；不得传递旧 Telegram secret。
- 保持现有发布逻辑与 artifact 行为不变。

## Acceptance

- `workflow_run` 在 `release` 失败时触发 Telegram 告警。
- `workflow_dispatch` 可手动发送 smoke test 通知。
- 失败与 smoke 告警 summary 都包含标题、项目名、状态、目标 SHA 与 run URL；首行保留
  `Emoji + 状态 + 项目名` 的既有格式，失败使用 `🚨 Release Failed`，smoke 使用 `🧪 Smoke Test`。
- 失败告警优先携带真实 release target SHA，而不是仅回退到 workflow 头 SHA。
- caller 只授予 reusable workflow 所需的 `id-token: write`，不传递 `SHOUTRRR_URL` 或其他旧
  notifier secret。
