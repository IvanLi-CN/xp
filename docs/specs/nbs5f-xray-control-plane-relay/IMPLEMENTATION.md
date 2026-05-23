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

## Remaining Gaps

- Docker/Xray live relay e2e remains a manual deployment validation because this feature is a control-plane transport option over the existing Xray process rather than a new dynamic Xray outbound reconciler.
- v1 does not implement L3 VPN, TUN/TAP, automatic full mesh addressing, or user subscription chaining changes.
