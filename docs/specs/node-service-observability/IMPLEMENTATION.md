# Implementation

## Current State

- The node-service observability behavior described in `SPEC.md` is implemented.
- `NodeDetailsPage` stores the selected tab in the route search and restores it on refresh.
- Node resource navigation carries the current valid tab when switching between node details.
  Non-node-detail links and invalid tab values remain unmodified.
- Node detail tabs render in this order on desktop and mobile: Service runtime, Resources,
  Node metadata, Traffic, IP usage, TCP connections, Node settings.
- Missing or invalid tab values default to Service runtime.
- Runtime aggregation, persistent status, and Web presentation remain maintained in their sources.

## Recorded Delivery State

- Status: 已完成
- Created: 2026-02-26
- Last: 2026-10-11

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: Backend runtime model + persistence + cloudflared supervisor
- [x] M2: Runtime admin/internal APIs + SSE forwarding + tests
- [x] M3: Web 页面改造 + Storybook/Vitest + 文档同步
