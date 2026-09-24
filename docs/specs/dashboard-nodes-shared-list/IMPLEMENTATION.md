# Implementation

## Current State

- The shared Dashboard and Nodes inventory list described in `SPEC.md` is implemented.
- Current UI behavior and coverage remain owned by the Web source and tests.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-03-03
- Last: 2026-03-04

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建共享 `NodeInventoryList`，覆盖桌面表格 + 移动卡片 + 图标纯链接行为
- [x] M2: Home/Nodes 页面统一切换到 runtime 查询并复用共享列表
- [x] M3: 完成测试与 mock 同步，验证 lint/typecheck/test 全通过
- [x] M4: 快车道完成 PR + checks + review-loop 收敛并回写 spec 状态
