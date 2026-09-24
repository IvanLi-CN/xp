# Implementation

## Current State

- The Grant-groups hard cut described in `SPEC.md` is implemented.
- The current access model is maintained by its successor access contracts and product sources.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-02-27
- Last: 2026-02-27

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 后端 domain/state/raft 去 group 化 + v6 迁移
- [x] M2: 后端 admin API 切换到 user-grants，grant-groups 下线
- [x] M3: 前端路由/页面/API/Storybook 去 group 化并接入 user-grants
- [x] M4: 全量验证通过并完成快车道收敛（PR + checks + review-loop）
