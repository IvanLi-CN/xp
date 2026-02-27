# 规格（Spec）总览

本目录用于管理工作项的**规格与追踪**：记录范围、验收标准、任务清单与状态，作为交付依据；实现与验证应以对应 `SPEC.md` 为准。

> Legacy compatibility: historical repos may still contain `docs/plan/**/PLAN.md`. New entries must be created under `docs/specs/**/SPEC.md`.

## 快速新增一个规格

1. 生成一个新的规格 `ID`（推荐 5 个字符的 nanoId 风格，降低并行建规格时的冲突概率）。
2. 新建目录：`docs/specs/<id>-<title>/`（`<title>` 用简短 slug，建议 kebab-case）。
3. 在该目录下创建 `SPEC.md`（模板见下方“SPEC.md 写法（简要）”）。
4. 在下方 Index 表新增一行，并把 `Status` 设为 `待设计` 或 `待实现`（取决于是否已冻结验收标准），并填入 `Last`（通常为当天）。

## 目录与命名规则

- 每个规格一个目录：`docs/specs/<id>-<title>/`
- `<id>`：推荐 5 个字符的 nanoId 风格，一经分配不要变更。
  - 推荐字符集（小写 + 避免易混淆字符）：`23456789abcdefghjkmnpqrstuvwxyz`
  - 正则：`[23456789abcdefghjkmnpqrstuvwxyz]{5}`
  - 兼容：若仓库历史已使用四位数字 `0001`-`9999`，允许继续共存。
- `<title>`：短标题 slug（建议 kebab-case，避免空格与特殊字符）；目录名尽量稳定。
- 人类可读标题写在 Index 的 `Title` 列；标题变更优先改 `Title`，不强制改目录名。

## 状态（Status）说明

仅允许使用以下状态值：

- `待设计`：范围/约束/验收标准尚未冻结，仍在补齐信息与决策。
- `待实现`：规格已冻结，可开工；实现与测试验证应以该规格为准。
- `跳过`：计划已冻结或部分完成，但**当前明确不应自动开工**（例如需要特定时机/外部条件/等待依赖）；自动挑选“下一个规格”时应跳过它。需要实现时再把状态改回 `待实现`（或由主人显式点名实现该规格）。
- `部分完成（x/y）`：实现进行中；`y` 为该规格里定义的“实现里程碑”数，`x` 为已完成“实现里程碑”数（见该规格 `SPEC.md` 的 Milestones；不要把计划阶段产出算进里程碑）。
- `已完成`：该规格已完成（实现已落地或将随某个 PR 落地）；如需关联 PR 号，写在 Index 的 `Notes`（例如 `PR #123`）。
- `作废`：不再推进（取消/价值不足/外部条件变化）。
- `重新设计（#<id>）`：该规格被另一个规格取代；`#<id>` 指向新的规格编号。

## `Last` 字段约定（推进时间）

- `Last` 表示该规格**上一次“推进进度/口径”**的日期，用于快速发现长期未推进的规格。
- 仅在以下情况更新 `Last`（不要因为改措辞/排版就更新）：
  - `Status` 变化（例如 `待设计` -> `待实现`，或 `部分完成（x/y）` -> `已完成`）
  - `Notes` 中写入/更新 PR 号（例如 `PR #123`）
  - `SPEC.md` 的里程碑勾选变化
  - 范围/验收标准冻结或发生实质变更

## SPEC.md 写法（简要）

每个规格的 `SPEC.md` 至少应包含：

- 背景/问题陈述（为什么要做）
- 目标 / 非目标（做什么、不做什么）
- 范围（in/out）
- 需求列表（MUST/SHOULD/COULD）
- 功能与行为规格（Functional/Behavior Spec：核心流程/关键边界/错误反馈）
- 验收标准（Given/When/Then + 边界/异常）
- 实现前置条件（Definition of Ready / Preconditions；未满足则保持 `待设计`）
- 非功能性验收/质量门槛（测试策略、质量检查、Storybook/视觉回归等按仓库已有约定）
- 文档更新（需要同步更新的项目设计文档/架构说明/README/ADR）
- 实现里程碑（Milestones，用于驱动 `部分完成（x/y）`；只写实现交付物，不要包含计划阶段产出）
- 风险与开放问题（需要决策的点）
- 假设（需主人确认）

## Index（固定表格）

|    ID | Title                                      | Status | Spec                                           | Last       | Notes                               |
| ----: | ------------------------------------------ | ------ | ---------------------------------------------- | ---------- | ----------------------------------- |
| s2w9k | Quota Policy 节点比例可视化编辑 + 映射替换 | 已完成 | `s2w9k-quota-policy-node-ratio-editor/SPEC.md` | 2026-02-26 | PR #81                              |
| 9vmap | 节点服务可观测升级                         | 已完成 | `9vmap-node-service-observability/SPEC.md`     | 2026-02-26 | local implementation + tests passed |
| yzqn6 | Remove grant groups hard cut               | 已完成 | `yzqn6-remove-grant-groups-hard-cut/SPEC.md`   | 2026-02-27 | PR #84                              |
