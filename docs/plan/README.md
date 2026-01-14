# 计划（Plan）总览

本目录用于管理“先计划、后实现”的工作项：每个计划在这里冻结范围与验收标准，进入实现前先把口径对齐，避免边做边改导致失控。

## 快速新增一个计划

1. 分配一个新的四位编号 `ID`（查看下方 Index，取未使用的最小或递增编号）。
2. 新建目录：`docs/plan/<id>:<title>/`（`<title>` 用简短 slug，建议 kebab-case）。
3. 在该目录下创建 `PLAN.md`（模板见下方“PLAN.md 写法（简要）”）。
4. 在下方 Index 表新增一行，并把 `Status` 设为 `待设计` 或 `待实现`（取决于是否已冻结验收标准），并填入 `Last`（通常为当天）。

## 目录与命名规则

- 每个计划一个目录：`docs/plan/<id>:<title>/`
- `<id>`：四位数字（`0001`–`9999`），一经分配不要变更。
- `<title>`：短标题 slug（建议 kebab-case，避免空格与特殊字符）；目录名尽量稳定。
- 人类可读标题写在 Index 的 `Title` 列；标题变更优先改 `Title`，不强制改目录名。

## 状态（Status）说明

仅允许使用以下状态值：

- `待设计`：范围/约束/验收标准尚未冻结，仍在补齐信息与决策。
- `待实现`：计划已冻结，允许进入实现阶段（或进入 PM/DEV 交付流程）。
- `部分完成（x/y）`：实现进行中；`y` 为该计划里定义的里程碑数，`x` 为已完成里程碑数（见该计划 `PLAN.md` 的 Milestones）。
- `待验收`：实现已完成，等待按验收标准进行验证与确认。
- `已完成`：该计划已完成（实现已落地或将随某个 PR 落地）；如需关联 PR 号，写在 Index 的 `Notes`（例如 `PR #123`）。
- `作废`：不再推进（取消/价值不足/外部条件变化）。
- `重新设计（#<id>）`：该计划被另一个计划取代；`#<id>` 指向新的计划编号。

## `Last` 字段约定（推进时间）

- `Last` 表示该计划**上一次“推进进度/口径”**的日期，用于快速发现长期未推进的计划。
- 仅在以下情况更新 `Last`（不要因为改措辞/排版就更新）：
  - `Status` 变化（例如 `待验收` → `已完成`）
  - `Notes` 中写入/更新 PR 号（例如 `PR #123`）
  - `PLAN.md` 的里程碑勾选变化
  - 范围/验收标准冻结或发生实质变更

## PLAN.md 写法（简要）

每个计划的 `PLAN.md` 至少应包含：

- 背景/问题陈述（为什么要做）
- 目标 / 非目标（做什么、不做什么）
- 范围（in/out）
- 需求列表（MUST/SHOULD/COULD）
- 验收标准（Given/When/Then + 边界/异常）
- 非功能性验收/质量门槛（测试策略、质量检查、Storybook/视觉回归等按仓库已有约定）
- 文档更新（需要同步更新的项目设计文档/架构说明/README/ADR）
- 里程碑（Milestones，用于驱动 `部分完成（x/y）`）
- 风险与开放问题（需要决策的点）

## Index（固定表格）

|   ID | Title                                                 | Status | Plan                                        | Last       | Notes |
| ---: | ----------------------------------------------------- | ------ | ------------------------------------------- | ---------- | ----- |
| 0001 | MVP 大纲（目标与里程碑）                              | 已完成 | `0001:mvp-outline/PLAN.md`                  | 2025-12-23 | -     |
| 0002 | Milestone 1 · 控制面基础（单机可用）                  | 已完成 | `0002:m1-control-plane-foundation/PLAN.md`  | 2025-12-17 | -     |
| 0003 | Milestone 2 · Xray Controller + Reconcile（单机闭环） | 已完成 | `0003:m2-xray-controller-reconcile/PLAN.md` | 2025-12-18 | -     |
| 0004 | Milestone 3 · 订阅输出（对用户可交付）                | 已完成 | `0004:m3-subscription-output/PLAN.md`       | 2025-12-18 | -     |
| 0005 | Milestone 4 · 配额系统（单机强约束）                  | 已完成 | `0005:m4-quota-system/PLAN.md`              | 2025-12-19 | -     |
| 0006 | Milestone 5 · Raft 集群（强一致 + HTTPS-only）        | 已完成 | `0006:m5-raft-cluster/PLAN.md`              | 2025-12-23 | -     |
| 0007 | Milestone 5 收尾 · Quota 强约束与 Raft 一致性         | 已完成 | `0007:m5-quota-raft-consistency/PLAN.md`    | 2025-12-21 | -     |
| 0008 | Milestone 6 · Web 面板（基础功能完整：CRUD）          | 已完成 | `0008:m6-web-panel/PLAN.md`                 | 2025-12-22 | -     |
| 0009 | Milestone 7 · 质量门禁与交付                          | 已完成 | `0009:m7-quality-gates-delivery/PLAN.md`    | 2025-12-23 | -     |
| 0010 | 控制面 Web UI 重设计（Geek 风格 + Light/Dark）        | 待实现 | `0010:admin-ui-geek-redesign/PLAN.md`       | 2026-01-13 | -     |
| 0011 | Storybook 启动不自动打开浏览器                        | 待实现 | `0011:storybook-no-auto-open/PLAN.md`       | 2026-01-14 | 默认 `--no-open` |
