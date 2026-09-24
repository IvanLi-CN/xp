# Implementation

## Current State

- The endpoint-probe denominator uses online participating nodes as specified in `SPEC.md`.
- Historical samples retain the documented fallback behavior for compatible aggregation.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-03-11
- Last: 2026-07-28

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 增加 per-hour participant 持久化与兼容写入路径
- [x] M2: 切换 endpoint summary/history 与 web 页面到 participant 分母
- [x] M3: 补齐 legacy fallback、Rust/Vitest tests、ops/spec 文档同步
