# Implementation

## Backend

- `src/node_history.rs` owns the local mirror file `${XP_DATA_DIR}/node_history_cache.json`.
- Local sampling runs hourly and records node-level Xray uplink/downlink deltas, daily component snapshots, and runtime `status_changed` events.
- Remote sync runs hourly and pulls `GET /api/admin/_internal/nodes/history/local` from other nodes with internal signature auth.
- Admin API exposes `GET /api/admin/nodes/{node_id}/history` for Web fallback reads.

## Frontend

- `web/src/api/adminNodeHistory.ts` defines the history response schema and fetcher.
- `NodeDetailsPage` keeps live runtime as the primary source; when live runtime fails and history exists, it renders the fallback history panel.
- `NodeDetailsPage.stories.tsx` includes `RuntimeHistoryFallback` with deterministic mock history and forced runtime failure.

## Validation

- `cargo test node_history`
- `cd web && bun run typecheck`
- `cd web && bun run test -- adminNodeHistory NodeDetailsPage`

## Recorded Delivery State

- Status: 已完成
- Created: 2026-05-20
- Last: 2026-05-20

## 实现里程碑

- [x] M1: Backend history model, persistence, local sample, remote sync and APIs
- [x] M2: Web API schema, NodeDetails fallback UI, tests and Storybook state
- [x] M3: Docs, validation, visual evidence and PR convergence
