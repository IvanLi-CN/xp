# PR intent labels：用标签驱动 Release 自动发版（#z27vg）

## 状态

- Status: 待实现
- Created: 2026-02-05
- Last: 2026-02-05

## 1) 背景 / 问题陈述

当前仓库已具备 main 分支的自动发布（GitHub Actions `release.yml`），但“是否发版 / 如何 bump 版本”的意图仍主要隐含在合并行为里：

- 文档类 PR / 纯重构 PR 也可能触发发版（或需要人为规避）。
- 版本递增规则缺少显式的“意图来源”，review 时不够直观。

本计划引入 **PR intent labels**：通过 PR 标签明确表达发版意图（skip/docs/patch/minor/major），并让 release workflow 以标签为唯一决策来源。

## 2) 目标 / 非目标

### Goals

- 引入一组互斥的 intent labels，并在 PR 层强制校验（缺失/冲突即 CI 失败）。
- release workflow 在 main 合并后：
  - 能定位合并对应的 PR；
  - 读取 intent label；
  - 决定是否发布，以及进行 major/minor/patch bump；
  - 创建（或更新）GitHub Release，并上传现有约定的 release assets。
- 失败策略保守：当无法唯一定位 PR 或 intent label 不合法时，**跳过发布**并给出可诊断原因。

### Non-goals

- 不引入 prerelease（alpha/beta/rc）通道与额外标签体系。
- 不改变 `xp-ops` 的升级消费协议（assets 命名、checksums 格式等维持现状）。
- 不引入复杂的版本回写（例如改写 `Cargo.toml`）。

## 3) 范围（In / Out）

### In scope

- GitHub labels（repo-level）：
  - `type:docs`：不发版（文档/注释/README 等）
  - `type:skip`：不发版（需要跳过发版的维护类变更）
  - `type:patch`：发版，patch + 1
  - `type:minor`：发版，minor + 1（patch = 0）
  - `type:major`：发版，major + 1（minor = 0, patch = 0）
- PR label gate：保证 PR 上述标签 **恰好 1 个**。
- Release intent 解析脚本：commit SHA -> PR -> labels -> should_release/bump_level。
- 版本计算脚本：从现有 semver tags 计算下一版本，并确保 tag 唯一。
- release workflow：按 intent label gate 发布与跳过，并保持可重试（幂等）。

### Out of scope

- 自动给 PR 打标签（保持人为明确表达意图）。
- “无 PR 的直接 push”自动发版（默认保守跳过）。

## 4) 验收标准（Acceptance Criteria）

- Given PR 目标分支为 `main`，
  When PR 没有 intent label / 同时存在多个 intent labels / 存在未知的 `type:*`，
  Then `PR Label Gate` workflow 必须失败，并给出可读错误信息。

- Given PR intent label 为 `type:docs` 或 `type:skip`，
  When 该 PR 合并到 `main` 且主 CI 通过，
  Then release workflow 必须 **不创建新 tag、不创建新 GitHub Release**（以日志/summary 标注“skip”原因）。

- Given PR intent label 为 `type:patch`，
  When 该 PR 合并到 `main` 且主 CI 通过，
  Then 必须创建一个新的 `vX.Y.Z` tag（Z 相对当前最大 tag 递增），并发布 GitHub Release（含现有约定 assets）。

- Given PR intent label 为 `type:minor` / `type:major`，
  When 该 PR 合并到 `main` 且主 CI 通过，
  Then 必须按语义化版本规则生成新的 `vX.Y.Z` 并发布（`minor` -> `X.(Y+1).0`，`major` -> `(X+1).0.0`）。

- Given 同一个 workflow run 被 rerun（或因重试触发重复执行），
  When tag / release 已存在，
  Then workflow 不应因“已存在”失败；应保持幂等（允许更新 release 并替换 assets）。

## 5) 测试与验证（Testing）

- 本地最小验证：
  - `bash -n` 校验新增/修改的 `.github/scripts/*.sh` 语法；
  - `bunx --no-install dprint check` 校验 Markdown/YAML 格式（若仓库约定启用）。
- CI/Actions：
  - 通过 PR 触发 `PR Label Gate` 验证 gate 行为；
  - 通过 `type:docs` 与 `type:patch` 各跑一遍主流程（可在合并到 main 后由 Actions 结果验证）。

## 6) 里程碑（Milestones）

- [ ] M1: 创建 intent labels + PR label gate
- [ ] M2: release intent 脚本 + 版本计算脚本
- [ ] M3: release workflow 按 labels 发布/跳过 + 幂等性处理
