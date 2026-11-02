# Implementation

## Current State

- The endpoint-probe denominator uses online participating nodes as specified in `SPEC.md`.
- Historical samples retain the documented fallback behavior for compatible aggregation.
- Probe-owned Xray child processes use `kill_on_drop`; on Linux they also receive
  `PR_SET_PDEATHSIG=SIGKILL`, so an abnormal XP exit cannot leave temporary probe processes
  consuming node memory.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-03-11
- Last: 2026-11-02

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 增加 per-hour participant 持久化与兼容写入路径
- [x] M2: 切换 endpoint summary/history 与 web 页面到 participant 分母
- [x] M3: 补齐 legacy fallback、Rust/Vitest tests、ops/spec 文档同步
- [x] M4: 绑定临时 Xray 子进程与 XP 生命周期，覆盖异常退出场景
