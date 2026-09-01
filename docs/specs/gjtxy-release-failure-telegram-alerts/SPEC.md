# gjtxy · Release 失败 Telegram 告警接入

## Context and Scope

XP 的 `release` 工作流需要在发布失败时发送 Telegram 告警，并保留一个可手动触发的
notifier smoke 路径。通知入口由 xp 自己的 wrapper 负责把发布上下文转换为 Oidrune 所需的
caller-owned summary。

本主题覆盖：

- `.github/workflows/notify-release-failure.yml` 的发布失败告警与手动 smoke workflow。
- Oidrune reusable workflow 的固定 commit SHA、OIDC 权限与默认 gateway 选择。
- failure 与 smoke summary 的项目语义、发布上下文和目标 SHA。
- `.github/scripts/test-notify-release-failure.py` workflow contract test 及其 CI 接入。
- `.github/workflows/release.yml` 的目标 SHA 日志标记兼容性；本次迁移不改变其发布逻辑与
  artifact 行为。

真实 Telegram 通知不属于本地或 PR 验证动作，必须由授权 operator 单独触发手动 smoke。

## Requirements

- REQ-NOTIFY-PIN: 两个通知 job 必须调用
  `IvanLi-CN/oidrune/.github/workflows/notify.yml` 的已核验固定 commit SHA，不得使用
  分支或 tag 引用。
- REQ-OIDC: caller 必须授予 reusable workflow 所需的 `id-token: write`，并省略
  `gateway_url` 与 `oidc_audience`，使 Oidrune 使用默认 gateway。
- REQ-SUMMARY: caller 必须完整生成 summary，至少包含项目名、状态、目标 SHA、run URL
  与 failure/smoke 标题，不得依赖 Oidrune 自动补充这些字段。
- REQ-FAILURE: `workflow_run` 必须继续只监听 `release` 在 `main` 上的 completed 事件，并
  只在 conclusion 为 `failure` 时运行失败通知；目标 SHA 解析与原有发布上下文必须保留。
- REQ-SMOKE: `workflow_dispatch` 必须继续提供手动 smoke 路径，并保持独立的 smoke 标题与
  `smoke test` 状态语义。
- REQ-LEGACY-SECRET: caller 不得继续传递旧 notifier 的 Telegram secret 或其他旧
  secret-based wiring。
- REQ-TITLE: failure 首行必须保持 `🚨 Release Failed · owner/repository` 格式，smoke 首行
  必须保持 `🧪 Smoke Test · owner/repository` 格式。

## Verification

- VER-CONTRACT covers: REQ-NOTIFY-PIN, REQ-OIDC, REQ-SUMMARY, REQ-FAILURE.
- VER-CONTRACT covers: REQ-SMOKE, REQ-LEGACY-SECRET, REQ-TITLE.
  `.github/scripts/test-notify-release-failure.py` 检查固定 SHA、权限、默认输入省略、secret 移除、
  触发过滤、失败判定、summary 字段和首行语义。
- VER-YAML covers: REQ-NOTIFY-PIN, REQ-OIDC, REQ-FAILURE, REQ-SMOKE. Ruby Psych 解析
  `notify-release-failure.yml` 与 `ci.yml`，确认 workflow YAML 结构有效。
- VER-CI covers: REQ-NOTIFY-PIN, REQ-OIDC, REQ-SUMMARY, REQ-FAILURE.
- VER-CI covers: REQ-SMOKE, REQ-LEGACY-SECRET, REQ-TITLE. `ci.yml` 在 Rust job 中执行
  workflow contract test。
- VER-LIVE covers: REQ-NOTIFY-PIN. 发布前通过 GitHub API 核对 Oidrune pinned SHA 的 commit、
  `main` 与最新可信 release tag 事实。

## Related ADRs

- None
