# Xray Stale TCP Reclaim 实现状态

## Current State

- Implemented and validation-complete for merge-ready.
- Static Xray baseline now writes the fixed `policy.levels.0` reclaim profile in `xp-ops init` and aligned dev/e2e fixtures.
- Dynamic business inbound builder now injects fixed `socket_settings` defaults for VLESS REALITY and SS2022 only.
- `xp-ops upgrade` now covers self-upgrade re-exec, static config rewrite, `xray` restart, and rollback-on-failure semantics for both config and `xp`.
- Static config rewrite preserves existing control-plane listener bindings: custom `XP_XRAY_API_ADDR` stays authoritative for the `api` inbound, and an existing `mesh-proxy` inbound is kept intact.

## Coverage Target

- Unit tests cover static config generation, inbound builder defaults, upgrade rollback behavior, and custom control-plane listener preservation during upgrade.
- Local quality gates cover `cargo test`, `cargo fmt --check`, and `cargo clippy -- -D warnings`.
- Shared testbox validation covers both ignored real-Xray suites in one isolated run.
