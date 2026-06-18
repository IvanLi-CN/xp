# Xray Stale TCP Reclaim 实现状态

## Current State

- Implemented and validation-complete for merge-ready.
- Static Xray baseline now writes the fixed `policy.levels.0` reclaim profile in `xp-ops init` and aligned dev/e2e fixtures.
- Dynamic business inbound builder now injects fixed `socket_settings` defaults for VLESS REALITY and SS2022 only.
- `xp-ops upgrade` now covers self-upgrade re-exec, static config rewrite, `xray` restart, and rollback-on-failure semantics for both config and `xp`.
- If a resumed post-self-upgrade phase fails, `xp-ops upgrade` now also restores the previous `xp-ops` binary instead of leaving the node on a half-upgraded operator binary.
- Static config rewrite preserves existing control-plane listener bindings: custom `XP_XRAY_API_ADDR` stays authoritative for the `api` inbound, and an existing `mesh-proxy` inbound is kept intact.
- The local real-Xray CI helper now pins `RUST_TEST_THREADS=1` so the ignored external-Xray suites do not race each other against one shared Xray process and forwarded SS port.
- The shared testbox runner now allocates an explicit free `10.203.x.0/24` subnet per isolated compose run and preflights host `make` availability before compiling vendored OpenSSL dependencies.
- Shared testbox subnet allocation is now serialized through a remote claim directory under `/srv/codex/shared-testbox/subnet-claims`, so concurrent real-Xray runs cannot select the same subnet between inspection and `docker compose up`.

## Coverage Target

- Unit tests cover static config generation, inbound builder defaults, upgrade rollback behavior, and custom control-plane listener preservation during upgrade.
- Local quality gates cover `cargo test`, `cargo fmt --check`, and `cargo clippy -- -D warnings`.
- Shared testbox validation covers both ignored real-Xray suites in one isolated run, with explicit subnet isolation and vendored-OpenSSL build prerequisites verified on the host.
