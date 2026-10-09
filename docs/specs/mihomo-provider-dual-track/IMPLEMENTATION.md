# Implementation

## Current State

- The provider-only Mihomo delivery contract in `SPEC.md` is implemented.
- Its managed-default endpoint behavior remains governed by the deployment contract.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-04-17
- Last: 2026-10-09

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: follow-up spec / 设计文档 / 契约冻结双轨语义
- [x] M2: provider-only admin config 与订阅路由
- [x] M3: provider 主配置 / payload 渲染 + origin 解析 helper
- [x] M4: provider payload 动态承载直连与链式节点
- [x] M5: Web 设置页与订阅 URL provider-only UI + Storybook
- [x] M6: 回归测试、视觉证据、共享测试机 Mihomo 验证
- [x] M7: PR / review / merge / cleanup

## Follow-up Requirements

The following rendering requirements were clarified after delivery and are now implemented.

- [x] Put every generated `🛬 {base}` Landing Group before the regional candidates in `🤯 All`.
- [x] Add other Subscription Nodes' `*-reality` Access Points to each system `🛣️` group.
      Exclude the Target Node, `DIRECT`, and unsubscribed nodes; use `REJECT` when none qualify.

- Provider rendering tests cover landing ordering, system-provider Reality filters,
  external provider additions, shared access-host exclusion, and fail-closed `REJECT` behavior.
