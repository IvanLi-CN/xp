# Prerelease E2E verification（#eu2rn）

## 状态

- Status: 待实现
- Created: 2026-02-05
- Last: 2026-02-05

## 背景 / 问题陈述

仓库已实现 prerelease（rc）发布通道（通过 PR label `channel:prerelease`），并已修复 release workflow 的 YAML 语法问题。当前需要按“正规流程”（CI 通过后自动发版）做一次端到端验证，确认：

- `channel:prerelease` 能稳定产出 `vX.Y.Z-rc.N` tag；
- GitHub Release 会被标记为 `prerelease=true`；
- release assets 与 `checksums.txt` 完整可用；
- `xp-ops upgrade --prerelease` 能正确选择最新 prerelease。

## 目标 / 非目标

### Goals

- 在不引入功能性变更的前提下，触发一次 prerelease（rc）发布并验证产物可用。
- 验证 `xp-ops upgrade --version latest --prerelease --dry-run` 能解析并选择该 rc 版本。

### Non-goals

- 不做任何线上环境调整（线上部署与域名配置另行推进）。
- 不修改 Cloudflare / 域名相关逻辑与配置（由 `xp-ops` + 配置文件负责）。

## 范围（Scope）

### In scope

- 新建一个最小 PR（变更可为 docs/注释级别的无风险改动），并打上以下 labels：
  - `type:patch`
  - `channel:prerelease`
- 合并到 `main` 后，等待 `ci` 通过并由 `release` workflow 自动创建 rc prerelease release。
- 验证 release 产物与 `xp-ops upgrade --prerelease --dry-run` 行为。

### Out of scope

- 不验证线上升级过程（仅验证“能选择到正确版本 + 产物齐全”）。

## 验收标准（Acceptance Criteria）

- Given PR labels 为 `type:patch` + `channel:prerelease`，
  When 合并到 `main` 且 `ci` workflow 通过，
  Then 必须出现一个新的 GitHub prerelease release：
  - `prerelease=true`
  - tag 形如 `vX.Y.Z-rc.N`
  - assets 包含 `xp-*`、`xp-ops-*` 与 `checksums.txt`（命名与既有约定一致）

- Given 上述 prerelease release 已创建，
  When 运行 `xp-ops upgrade --version latest --prerelease --dry-run`，
  Then 输出中应显示选择的目标版本为该 `X.Y.Z-rc.N`（或更新的 rc 版本）。

- Docker compose 回归环境（本地）：
  - `./scripts/dev/subscription-3node-compose/run.sh reset-and-verify` 通过。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- PR checks 必须全绿后才允许合并。
- 合并后检查 `release` workflow run 中：
  - release intent summary 显示 `is_prerelease=true` 且 `should_release=true`
  - 版本输出包含 `-rc.`

## 实现里程碑（Milestones）

- [ ] M1: 创建并合并带 `type:patch` + `channel:prerelease` 的最小 PR
- [ ] M2: `release` workflow 成功产出 prerelease release（tag + assets + checksums）
- [ ] M3: `xp-ops upgrade --prerelease --dry-run` 选择到最新 rc
- [ ] M4: docker compose 回归环境通过

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若 PR 被 squash/rebase 造成 merge commit 与 PR 映射异常，release intent 可能保守跳过发布（需保持默认 merge 行为）。
