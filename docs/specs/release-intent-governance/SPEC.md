# 发布意图治理

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，
> 关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

发布 workflow 过去读取合并后 PR 的当前标签，并只等待 `ci`。
标签在合并后可以变化，且 `fixture-policy` 与 `xray-e2e` 可能尚未完成，
因此同一 Release Target 的发布结果不稳定或过早。
该主题固定正常发布的意图来源，并为历史纠正提供可审计的人工入口。

## 目标 / 非目标

### Goals

- 以合并事件前的 GitHub Issue events 重建唯一 type/channel 意图。
- 以精确 SHA 等待 `ci`、`fixture-policy`、`xray-e2e` 的 main push 成功。
- 允许带 expected version 和非敏感 reason 的 owner-approved Manual Backfill。

### Non-goals

- 不创建仓库内意图快照、账本、队列或外部记录。
- 不修改已合并 PR 的历史标签或合并提交。

## 范围（Scope）

### In scope

- PR label gate、release intent resolver、workflow dispatch 输入、readiness gate、
  失败通知和发布运维文档。

### Out of scope

- 版本算法本身、镜像构建内容、部署运行时和 Cloudflare 行为。

## 需求（Requirements）

### MUST

- PR 必须且只能有一个受管 `type:docs|skip|patch|minor|major` 与一个受管
  `channel:stable|prerelease`。
- 自动 resolver 必须只重放合并事件及其之前的标签事件；事件 API 必须按 `Link`
  关系完整分页并包含唯一 merge event；缺失/重复/未知标签、无法唯一映射 PR
  或 API 失败必须 fail closed。
- 需要发布时，三个指定 workflow 必须是目标 SHA 的 `push`、`main`、`success` run。
- Manual Backfill 必须提供 `head_sha`、`release_type`、`channel`、
  `expected_version`、`reason`，且计算版本必须精确等于 expected version。
- 发布 tag 必须指向 Release Target；同名 tag 指向其他提交时拒绝。

### SHOULD

- Readiness gate 在工作流仍运行时轮询，并在失败或超时时输出各 workflow
  的 run 诊断。
- 失败通知携带 intent source/type/channel、目标 SHA 和 backfill reason，
  但不得携带凭据或请求 body。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 自动路径：`ci` 成功触发 release，映射唯一已合并 PR，重放其 merge event
  前的标签，若为 patch/minor/major 则等待三条精确 SHA readiness 后发布。
- 人工路径：验证目标是 `origin/main` 的已合并提交，使用显式输入计算版本。
  expected version 不匹配时在 tag 前失败；同目标已有正确 tag 时允许重试。
- docs/skip 意图只记录 skip，不创建 tag、GitHub Release 或 GHCR 产物。

### Edge cases / errors

- 合并后新增或删除的标签不得改变自动意图。
- 目标 SHA 没有唯一 merged PR、事件时间线无法证明唯一 merge event、完整分页或事件
  时间戳、
  readiness 缺失/失败/取消/跳过/超时，均停止发布。
- stable 不接受 prerelease expected version，prerelease 必须使用 `-rc.N` expected version。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

- `release dispatch`: internal workflow input, modified; consumed by the owner;
  it is the explicit Manual Backfill entry point.
- `release intent outputs`: internal workflow output, modified; consumed by the
  release and notifier jobs for source, type, channel, and target context.

### 契约文档（按 Kind 拆分）

- `None`。

## 验收标准（Acceptance Criteria）

- Given a PR whose labels change after merge, When release resolves its target,
  Then only labels replayed through the merge event affect the result.
- Given a release target, When any required main push workflow is missing,
  non-success, or for another SHA, Then no tag or artifact is published.
- Given a Manual Backfill, When computed version differs from expected_version,
  Then the workflow fails before tag creation.

## 实现前置条件（Definition of Ready / Preconditions）

- `channel:stable` 与 `channel:prerelease` 标签已存在并可由 PR label gate 读取。
- 发布 token 具有现有 `contents:write`、`packages:write`、`issues:read`、
  `pull-requests:read` 和 `actions:read` 权限。
- 目标仓库的 `main` 分支与三个 required workflow 名称保持稳定。

## 文档更新

- 维护本主题的 `SPEC.md`、`IMPLEMENTATION.md`、`HISTORY.md` 与 Spec catalog。
- 维护根 `CONTEXT.md`、`docs/adr/0001-merge-time-release-intent.md`、
  `docs/ops/README.md` 和 `docs/ops/docker.md` 的发布真相。

## 实现里程碑（Milestones）

- 唯一 type/channel label gate 和 merge-time event replay 可独立验证。
- 精确 SHA readiness 与 Manual Backfill expected-version guard 可独立验证。
- 治理 PR 合并后，目标 SHA 的 stable Release、checksums 和 GHCR 镜像完成验证。

## 验收清单（Acceptance checklist）

- [ ] 合并时意图与 type/channel 门禁已明确。
- [ ] 精确 SHA readiness、失败分类和超时行为已明确。
- [ ] Manual Backfill 输入、版本校验和幂等 tag 行为已明确。
- [ ] 历史 PR 不改写且不新增自有意图记录已明确。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: event replay, label validation, manual input validation, readiness evaluation.
- Integration tests: GitHub workflow checks and release run on the corrected target SHA.

### UI / Storybook (if applicable)

- `PR: none`

### Quality checks

- Shell syntax, Python unittest, dprint, workflow required checks, and code review must pass.

## Visual Evidence

PR: none

## Related PRs

- #267 — Reality Mesh Reverse Relay target requiring corrective release.

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：GitHub events API 的历史分页上限可能隐藏很旧的标签事件；resolver
  在无法证明完整 merge history 时 fail closed。
- 假设：发布 token 具有现有 contents/packages 写权限和 actions read 权限。

## 参考（References）

- `docs/solutions/ci/release-backfill-version-resolution.md`
- `.github/workflows/release.yml`
