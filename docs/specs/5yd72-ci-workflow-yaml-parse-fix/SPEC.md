# CI workflow YAML 语法修复（#5yd72）

## 状态

- Status: 部分完成（2/3）
- Created: 2026-03-05
- Last: 2026-03-05

## 背景 / 问题陈述

- `main` 在合入 PR #97 后，`ci` 工作流报错：`.github/workflows/ci.yml#L30` YAML 语法无效。
- `release` 工作流依赖 `ci` 成功（`workflow_run.conclusion == success`），因此被阻断，无法继续自动发版。
- 线上升级流程依赖 GitHub Releases，发布链路阻断会直接影响生产升级节奏。

## 目标 / 非目标

### Goals

- 修复 `ci.yml` 语法错误，恢复 `ci` 可运行状态。
- 确保 `release` 工作流重新可触发并可完成发版。
- 保持现有 CI 检查逻辑不变（仅做语法/结构层修复）。

### Non-goals

- 不调整业务代码与功能行为。
- 不重构 CI job 结构与测试矩阵。

## 范围（Scope）

### In scope

- `.github/workflows/ci.yml` 的 YAML 语法修复。
- 相关最小验证（YAML 可解析 + CI/checks 远端状态收敛）。
- 同步规格状态与变更记录。

### Out of scope

- 新增/删除 CI job。
- 修改 release 版本策略与 semver 规则。

## 需求（Requirements）

### MUST

- `ci.yml` 在标准 YAML 解析器下通过语法解析。
- PR checks 中 `ci` 状态明确且非“工作流文件无效”。
- 修复后不引入新的 workflow 语法错误。

## 验收标准（Acceptance Criteria）

- Given 当前 `ci.yml` 第 30 行含 `mapping values are not allowed in this context`，
  When 应用修复并触发 CI，
  Then `ci` 不再因 YAML 语法报错失败。
- Given `release` 依赖 `ci` 成功，
  When `ci` 通过，
  Then `release` 可继续执行并恢复自动发版链路。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 定位并修复 `ci.yml` 语法问题
- [x] M2: 本地完成 YAML 语法验证
- [ ] M3: PR checks / release 链路状态明确

## 风险 / 开放问题 / 假设

- 风险：若存在其他隐藏 YAML/动作配置错误，可能在修复单点后继续失败。
- 假设：阻断主因确认为 `ci.yml` 行内语法错误。

## 变更记录（Change log）

- 2026-03-05: 创建规格，冻结“仅修复 workflow 语法并恢复发版链路”的范围。
- 2026-03-05: 完成 `ci.yml` 语法修复与本地 YAML 解析验证（M1/M2）。
