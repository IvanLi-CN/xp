# Release workflow YAML syntax fix（#jwzaq）

## 状态

- Status: 已完成
- Created: 2026-02-05
- Last: 2026-02-05
- Notes: PR #65

## 背景 / 问题陈述

当前 `release` GitHub Actions workflow 被 GitHub 判定为 **YAML 语法错误**（plain scalar 中包含 `:`），导致：

- `release.yml` 无法被加载；
- workflow run 直接失败且没有 jobs（表现为 “jobs=0 / logs=404”）；
- 进而阻断 “stable / prerelease(rc)” 的正规发版通道验证。

## 目标 / 非目标

### Goals

- 修复 `.github/workflows/release.yml` 的 YAML 语法，使 workflow 可被 GitHub 正常解析与执行。
- 不改变现有 release 逻辑（release intent、版本计算、assets 命名等均保持不变）。

### Non-goals

- 不调整发版策略或标签规则（`type:*` / `channel:prerelease` 等均不在本计划变更范围）。

## 范围（Scope）

### In scope

- 将 `skip release (intent says no)` step 的 `run:` 由 plain scalar 改为 quoted scalar，避免 `:` 触发 YAML 解析失败。

### Out of scope

- 任何与 Cloudflare / 域名配置相关的改动（由 `xp-ops` 与配置文件负责）。

## 验收标准（Acceptance Criteria）

- Given `.github/workflows/release.yml` 已修复，
  When 该变更合并到 `main` 且 CI 流程触发，
  Then GitHub Actions 不应再出现 “Invalid workflow file” 的语法错误。

- Given 合并该 PR 且 intent label 为 `type:skip`，
  When release workflow 被触发，
  Then workflow 应正常运行并明确输出 “skip release: intent says no”，且不应创建 tag / release。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- 本地：YAML 可被解析（例如 Ruby/Psych `YAML.load_file`）。
- 远端：PR checks 通过；合并后 release workflow 运行状态明确（不再 jobs=0）。

## 实现里程碑（Milestones）

- [x] M1: 修复 release workflow YAML 语法并通过 PR checks
- [x] M2: 合并到 main 后验证 release workflow 可正常执行（至少能跑到 “skip release”）

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若 GitHub Actions UI 缓存旧的 workflow 解析结果，可能需要等待下一次触发才能确认恢复。
