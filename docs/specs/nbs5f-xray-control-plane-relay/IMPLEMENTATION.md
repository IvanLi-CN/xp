# Xray 控制面 Relay 实现状态

## Current State

- Implemented.
- `XP_MESH_PROXY_URL` / `--mesh-proxy-url` enables a relay-aware control-plane HTTP client.
- Raft RPC, internal admin fan-out, runtime fan-out, IP usage fan-out, node egress probe refresh, endpoint probe fan-out, alerts, and quota detail aggregation now use the shared relay-aware client.
- `xp-ops init` writes a loopback-only Xray SOCKS inbound at `127.0.0.1:10808` with tag `mesh-proxy`.
- `/api/health` and `/api/admin/config` expose mesh proxy status and fallback information.

## Coverage

- `cargo test`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- Static Xray config test covers the `mesh-proxy` inbound and route.
- Unit tests cover proxy URL validation and stable status strings.
- Shared testbox cluster validation covers a 3-node control-plane setup with Xray SOCKS relay enabled:
  - node2 `mesh_proxy_status=ready`
  - node2 `/api/admin/nodes/runtime` fan-out succeeds through the relay-aware client
  - stopping node2's Xray proxy keeps runtime fan-out working through direct fallback
  - node2 `mesh_proxy_status=fallback` after the fallback path is used

## Remaining Gaps

- v1 does not implement L3 VPN, TUN/TAP, automatic full mesh addressing, or user subscription chaining changes.
