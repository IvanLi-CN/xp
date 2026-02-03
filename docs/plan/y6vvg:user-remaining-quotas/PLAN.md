# Web UI：User 剩余额度展示（#y6vvg）

## 状态

- Status: 待实现
- Created: 2026-02-03
- Last: 2026-02-03

## 背景 / 问题陈述

- 目前 Admin UI 的用户列表与用户详情页缺少“每个节点当前剩余额度”的展示，排查用户可用流量时需要切到别处或手动计算。
- 现有用户详情页已有 `Node quotas` tab，但命名容易与“展示各节点额度信息”的新 tab 产生歧义。

## 目标 / 非目标

### Goals

- 用户列表新增一列：汇总展示该用户在所有节点的 `剩余额度/当前额度`（总剩余 / 总额度）。
- 用户详情页 tabs 新增一个“各节点额度信息”tab：
  - 展示每个节点的 `剩余额度/当前额度`
  - 展示下次重置时间（包含友好文案，例如 `10天后`）
- 调整现有 `Node quotas` tab 的命名，避免与新 tab 混淆。

### Non-goals

- 不修改配额扣减/重置的后端语义与实现。
- 不在本计划内做全局配额报表/历史明细等“分析类”功能。

## 范围（Scope）

### In scope

- Web Admin UI：
  - User list（用户列表）新增“总额度”列，显示 `remaining/limit` 聚合值。
  - User details（用户详情）新增一个 tab 用于展示“各节点额度信息”（含 next reset）。
  - 重命名现有 `Node quotas` tab（矩阵配置入口）以消除歧义。
- API（如现有接口未包含所需字段）：补齐获取“remaining / next reset”所需数据。

### Out of scope

- 节点、用户、Grant 等 CRUD 的信息架构重做。
- 复杂的时区/日历规则自定义 UI（仅按后端提供的 reset 时间展示）。

## 需求（Requirements）

### MUST

- 用户列表新增列展示“总剩余/总额度”，可在不展开详情的情况下快速判断用户是否可用。
- 用户详情页新增 tab 展示“各节点额度”：
  - 每行一个 node，展示 `remaining/limit`
  - 展示下次重置时间与相对时间（例如 `10天后` / `3小时后`）
- 现有 `Node quotas`（矩阵配置）tab 改名为更明确的“配置类”名称，避免与“额度信息”tab 混淆。

### SHOULD

- 若某节点没有配置额度或无法计算 remaining，UI 以 `-` 兜底并在 tooltip 提示原因（例如“未配置配额”）。
- 数值展示保持与现有 quota editor 一致（bytes → GiB/MiB 的用户友好格式）。

## 验收标准（Acceptance Criteria）

- Given 我打开用户列表，
  When 列表渲染完成，
  Then 我能看到每个用户的“总剩余/总额度”列，并且数值为所有节点之和。
- Given 我打开某个用户详情页，
  When 我切到“各节点额度”tab，
  Then 我能看到每个节点的 `remaining/limit` 与下次重置时间（含相对时间文案）。
- Given 我查看 tabs 命名，
  When 页面渲染完成，
  Then “矩阵配置”tab 与“额度信息”tab 名称不冲突且语义明确。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Frontend：`cd web && bun run lint && bun run typecheck`
- （若引入了新计算逻辑）补充最小单测覆盖：合并多节点 remaining/limit 的边界（空集合/缺失字段）。

## 风险 / 开放问题（Risks / Open Questions）

- 风险：现有 API 若未暴露 “remaining / next reset”，需要补齐后端字段或新增接口；实现前应确认接口口径与时区来源。
