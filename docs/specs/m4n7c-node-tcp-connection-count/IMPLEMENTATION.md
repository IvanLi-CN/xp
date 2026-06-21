# Implementation

## Backend

- `src/tcp_connection_usage.rs` defines the Linux-only TCP connection history model, `/proc/net/tcp*` socket sampling helpers, warning semantics, and `24h` / `7d` window shaping.
- `src/state.rs` persists `${XP_DATA_DIR}/tcp_connection_usage.json`, normalizes stale endpoint metadata on load, and exposes clear/prune/record helpers for node and endpoint lifecycle events.
- `src/quota.rs` now samples local business endpoint listen ports once per minute, counting only socket-level `ESTABLISHED` inbound TCP connections and recording them independently from inbound IP usage.
- `src/http/mod.rs` exposes `GET /api/admin/nodes/{node_id}/tcp-connections` and `GET /api/admin/_internal/nodes/tcp-connections/local`, reusing existing local/remote node fan-out patterns.
- `src/raft/app.rs` and `src/raft/storage/file.rs` clear or prune TCP history when endpoints or nodes are deleted or replaced.

## Frontend

- `web/src/api/adminTcpConnections.ts` adds the dedicated schema and fetcher for node TCP connection history.
- `web/src/components/TcpConnectionUsageView.tsx` renders the independent TCP panel with `24h` / `7d` switching, endpoint multi-select, aggregate summaries, chart, and warning/unsupported/empty states.
- `web/src/views/NodeDetailsPage.tsx` adds the `TCP connections` tab and lazy-loads the new API independently from `IP usage`.
- `web/src/storybook/tcpConnectionStoryData.ts`, `web/src/components/TcpConnectionUsageView.stories.tsx`, and `web/src/views/NodeDetailsPage.stories.tsx` provide deterministic Storybook coverage for the new panel.

## Coverage

- Rust unit tests in `src/tcp_connection_usage.rs` cover history rotation, normalization, cleanup, and `/proc` parsing helpers.
- `src/http/tests.rs` covers node TCP connection API responses, invalid windows, and unsupported-platform warnings.
- Web coverage includes `web/src/components/TcpConnectionUsageView.test.tsx`, `web/src/api/adminTcpConnections.test.ts`, and `web/src/views/NodeDetailsPage.test.tsx`.

## Validation

- `cargo fmt --all`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run test-storybook`
