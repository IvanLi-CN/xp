# Implementation

## Current State

- The hard cut described in `SPEC.md` has no recorded completion evidence.
- Do not treat the planned access-model migration as delivered until its acceptance criteria pass.

## Delivery Scope

- The migration must remove Grant-group semantics in one controlled, compatibility-aware release.

## Recorded Delivery State

- Status: 待实现
- Created: 2026-02-27
- Last: 2026-02-27

## 实现里程碑

- [ ] M1: 冻结 docs/specs + contracts（Access API、迁移与兼容边界）
- [ ] M2: 后端完成 schema v9、access API、grant-group API 下线、WAL shim
- [ ] M3: 前端移除 Grant groups 面并接入 access API
- [ ] M4: Rust/Web/Storybook/E2E 全量回归通过
- [ ] M5: 快车道收口（PR + checks + review-loop）
