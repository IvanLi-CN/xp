# Xray Stale TCP Reclaim 实现状态

## Current State

- In progress.
- Static Xray baseline is being aligned to a fixed `policy.levels.0` reclaim profile.
- Dynamic business inbound builder is being aligned to fixed `socket_settings` defaults for VLESS REALITY and SS2022 only.
- `xp-ops upgrade` is being extended to cover self-upgrade, static config rewrite, `xray` restart, and rollback-on-failure semantics.

## Coverage Target

- Unit tests cover static config generation, inbound builder defaults, and upgrade rollback behavior.
- Local quality gates cover `cargo test`, `cargo fmt --check`, and `cargo clippy -- -D warnings`.
- Shared testbox validation must cover both ignored real-Xray suites in one isolated run.
