# Storybook 启动不自动打开浏览器（#0011）

## 状态

- Status: 待实现
- Created: 2026-01-14
- Last: 2026-01-14

## 1) 问题陈述

当前 `web` 的 Storybook 启动脚本为 `storybook dev -p 60081 --host 127.0.0.1`，未显式关闭“自动打开浏览器”的行为。根据 Storybook CLI 官方文档，`storybook dev` 支持 `--no-open`（禁止自动打开浏览器）以及 `--ci`（CI 模式：跳过交互提示且不打开浏览器）。因此在默认脚本不带 `--no-open/--ci` 的情况下，存在启动时自动拉起浏览器的风险；这会干扰开发者工作流，且在多工作区/多服务并行时尤其糟糕。

## 2) 目标 / 非目标

### Goals

- 默认执行 `cd web && bun run storybook` 时，不会自动打开任何浏览器窗口/Tab。
- Storybook 仍可正常启动并在控制台输出可访问 URL（开发者可自行打开）。
- 不改变端口（`60081`）与 host（`127.0.0.1`）等现有约束，避免影响既有 `test-storybook` / 文档约定。

### Non-goals

- 不更改 Storybook 版本、builder、预览配置与 stories 结构。
- 不引入新的依赖或新的脚本工具链（沿用现有 `bun` / Storybook CLI 能力）。
- 不在计划阶段修改任何实现代码或运行配置（仅冻结口径与验收标准）。

## 3) 用户与场景

- **主要用户**：Web UI 开发者、维护者。
- **典型场景**
  - 本地调试 UI 组件：频繁重启 Storybook，但不希望每次都自动弹浏览器。
  - 同时运行多个服务（dev server / storybook / test runner）：自动打开会打断焦点与窗口管理。
  - 在非 GUI 环境（CI / SSH / 远程容器）中运行：自动打开浏览器属于无意义副作用，应被默认禁用。

## 4) 需求列表（MUST/SHOULD/COULD）

### MUST

- `cd web && bun run storybook` 默认不自动打开浏览器。
- 默认脚本通过 `storybook dev --no-open` 达成上述行为（见「假设」与「里程碑」中的实现口径）。
- 仍然能从 CLI 明确获得访问地址（例如 `http://127.0.0.1:60081/`），且 dev server 正常运行。
- 不影响现有 `cd web && bun run test-storybook` 的行为与参数语义（其已包含 `--ci`，应继续保持）。

### SHOULD

- 如需要“自动打开浏览器”的体验，应提供一个明确、显式的替代入口（例如单独脚本或显式参数），避免把副作用塞进默认命令。

### COULD

- 在 `web/README.md`（或等价文档）补充“Storybook 不自动打开浏览器，需手动访问 URL”的说明。

## 5) 接口清单与契约（Inputs/Outputs/Errors）

本计划只触及仓库内部 CLI 接口（脚本命令口径），不涉及 HTTP API / DB / 文件格式等对外接口。

### 接口清单（Inventory）

| 接口（Name）             | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| ------------------------ | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ------------- |
| `bun run storybook`      | CLI          | internal      | Modify         | ./contracts/cli.md       | web             | 开发者 / CI         | 默认禁用 open |
| `bun run test-storybook` | CLI          | internal      | None           | ./contracts/cli.md       | web             | CI                  | 已使用 `--ci` |

## 6) 约束与风险

- 约束：计划阶段只落地文档；实现阶段再修改 `web/package.json` 的脚本参数。
- 风险：如团队中有人依赖“自动打开浏览器”作为默认行为，改动后需要明确替代入口（见 SHOULD）。
- 风险：不同 Storybook 版本的 CLI 行为可能存在差异；本仓库使用 `storybook@^8.0.0`，实现时需以安装后的实际版本为准（可通过 `storybook dev --help` 复核）。

## 7) 验收标准（Acceptance Criteria）

### 核心行为

- Given 本地环境具备可运行的 `web` 依赖，
  When 执行 `cd web && bun run storybook`，
  Then Storybook 服务正常启动且**不会**自动打开浏览器窗口/Tab，
  And CLI 输出中包含可访问的地址（host=`127.0.0.1`，port=`60081`）。

### 兼容性

- Given 已存在的 `cd web && bun run test-storybook` 流程，
  When 在本地或 CI 执行该命令，
  Then 行为与当前一致（仍为 `--ci` 模式，不打开浏览器），并能访问 `http://127.0.0.1:60081`。

## 8) 开放问题（需要主人回答）

None

## 9) 假设（需主人确认）

- 假设：对本仓库当前使用的 Storybook 版本而言，`storybook dev` 默认会尝试自动打开浏览器；可用 `--no-open` 禁用（以 Storybook CLI 文档为依据，且在实现阶段会通过 `storybook dev --help` 复核）。

## 非功能性验收 / 质量门槛（Quality Gates）

- 不新增/修改测试用例（此变更为脚本行为，验收以命令行为为主）。
- 实现阶段完成后，至少运行：
  - `cd web && bun run lint`
  - `cd web && bun run typecheck`
  - `cd web && bun run test-storybook`

## 文档更新（Docs to Update）

- （可选）`web/README.md`：补充 Storybook 启动口径（默认不自动打开浏览器，手动访问 URL）。

## 里程碑（Milestones）

- [x] M1: 冻结方案与口径（默认使用 `--no-open`；不提供 `storybook:open`）
- [ ] M2: 实现脚本变更并验证（本地 + CI 口径）
- [ ] M3:（可选）补充文档说明

## 参考（References）

- Storybook docs · CLI options：`--no-open` / `--ci`
