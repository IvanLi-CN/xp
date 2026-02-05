# Prerelease release channel：用标签产出 rc 预发布（#hshdf）

## 状态

- Status: 待实现
- Created: 2026-02-05
- Last: 2026-02-05

## 背景 / 问题陈述

当前仓库已具备 main 分支的自动发布工作流，并且 `xp-ops upgrade --version latest --prerelease` 已支持“选择最新 prerelease release”。但现有 release workflow 只会创建 stable release（不会设置 GitHub Release 的 `prerelease=true`），因此：

- 运维侧无法通过 `--prerelease` 拉取测试版本；
- PR 合并后的“先走预发布验证，再转 stable”缺少正规通道与可审计的意图来源。

## 目标 / 非目标

### Goals

- 引入一种 **显式且可审计** 的方式，让特定 PR 产出 prerelease（rc）发布：
  - PR 打上 `channel:prerelease` label 时，release workflow 产出 prerelease release（GitHub `prerelease=true`）。
- prerelease tag 使用 semver prerelease 后缀（建议 `vX.Y.Z-rc.N`）以避免与后续 stable `vX.Y.Z` 冲突。
- 保持稳定通道不变：未标记 `channel:prerelease` 时，行为与当前 stable release 完全一致。

### Non-goals

- 不新增 Web / 公开管理员 API 的发布通道控制能力（发布通道属于仓库与 CI 管理范畴）。
- 不引入多通道矩阵（alpha/beta/rc 多套体系）；本计划只做一个 `prerelease(rc)` 标记。
- 不改变 `xp-ops` 的消费协议（assets 命名、checksums 格式维持既有约定）。

## 范围（Scope）

### In scope

- GitHub labels（repo-level）：
  - 新增 `channel:prerelease`：表示该 PR 合并后应产出 prerelease release。
- release intent 解析脚本：
  - 在保持现有 `type:*` intent labels 规则不变的前提下，额外识别 `channel:prerelease` 并输出 `is_prerelease=true|false`。
- 版本计算脚本：
  - stable：继续生成 `X.Y.Z`。
  - prerelease：生成 `X.Y.Z-rc.N`（N 递增，保证 tag 唯一）。
- release workflow：
  - 将 `is_prerelease` 传递给 release action，并设置 `prerelease: true`。

### Out of scope

- 自动给 PR 打 `channel:prerelease`（仍由人明确表达意图）。
- 为 `xp-ops` 新增额外的“channel 参数”（已具备 `--prerelease`）。

## 需求（Requirements）

### MUST

- Given PR 同时包含：
  - 1 个合法 intent label（`type:patch|type:minor|type:major`），以及
  - `channel:prerelease`，
  When PR 合并到 `main` 且主 CI 通过，
  Then release workflow 必须创建 **prerelease release**，其 tag 必须为 `vX.Y.Z-rc.N`（保证不与 stable `vX.Y.Z` 冲突）。

- Given PR intent label 为 `type:docs` / `type:skip`，
  When PR 合并到 `main` 且主 CI 通过，
  Then 必须跳过发布（即使存在 `channel:prerelease`）。

- Given 同一个 workflow run 被 rerun，
  When tag / release 已存在，
  Then workflow 应保持幂等（允许更新 release 并替换 assets）。

### SHOULD

- 版本生成策略对人类可读：默认后缀为 `-rc.N`（N 从 1 开始递增）。

## 接口契约（Interfaces & Contracts）

本计划不新增对外 HTTP/CLI 接口；主要影响 “release tag 命名 + GitHub Release prerelease 标记”，供 `xp-ops` 消费。

- Release tag: `v<semver>`（允许 semver 包含 prerelease 段，例如 `v1.2.3-rc.1`）
- Assets naming / checksums：不变（见 `docs/plan/0018:xp-ops-upgrade-and-release-automation/contracts/file-formats.md`）

## 验收标准（Acceptance Criteria）

- Given PR 被打上 `type:patch` + `channel:prerelease`，
  When 合并到 `main` 且主 CI 通过，
  Then 仓库必须出现一个新的 prerelease release：
  - `prerelease=true`
  - tag 形如 `vX.Y.Z-rc.N`
  - assets 与 `checksums.txt` 齐全且命名符合既有约定

- Given PR 仅有 `type:patch`（不含 `channel:prerelease`），
  When 合并到 `main` 且主 CI 通过，
  Then 必须创建 stable release（与当前行为一致）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- 本地最小验证：
  - `bash -n .github/scripts/*.sh`
  - 用临时 git repo 验证 version 脚本在：
    - 无 rc tag 时产出 `-rc.1`
    - 已有 `-rc.1` 时产出 `-rc.2`

### Quality checks

- 不引入新工具；维持仓库既有 CI 质量门槛不下降。

## 实现里程碑（Milestones）

- [ ] M1: 新增 `channel:prerelease` label，并让 release intent 解析脚本输出 `is_prerelease`
- [ ] M2: 版本计算脚本支持 `X.Y.Z-rc.N`，release workflow 设置 `prerelease: true`
- [ ] M3: 文档/计划同步（必要时补齐 contracts 的 tag 说明）

