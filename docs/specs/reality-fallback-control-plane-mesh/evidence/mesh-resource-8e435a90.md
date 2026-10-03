# Mesh Resource Evidence

- Candidate commit: `8e435a900f93da8a2f30ed15fd38437337aafec1`
- Evidence persistence: this record binds the candidate source and generated Web shell to the
  immutable shared-testbox run below.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260930_141513_8e435a900f93_8b284427`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@ed109323`
- Source archive SHA-256: `99d0988ee11e299451523aaab5674e901593733a4e5c9f4aa88caec1773a27e0`
- Generated Web archive SHA-256: `ca3646ae3a5b145743853a4da4c8b1944e810a3bac08668ee3f4461140f5f6bc`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- 50-peer baseline/candidate peak PSS: `33789/33112 KiB`
- 50-peer baseline/candidate anonymous PSS: `16888/17060 KiB`
- 50-peer baseline/candidate stack PSS: `43681/43028 KiB`
- 50-peer baseline/candidate CPU ticks: `202/207`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `27900 KiB`
- Source journal peak PSS: `27426 KiB`; CPU p95: `1%`; additional read bytes: `0`; max RSS
  delta: `65536 bytes`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke reached all 50 peers before the formal sample; formal workload also observed 50
  persistent H2 connections.
- Cargo build phases completed in `401s` (candidate), `462s` (baseline), and `399s` (resource test).
- Evidence manifest:
  `/tmp/xp-testbox-evidence-8e435a90-final/20260930_141513_8e435a900f93_8b284427.manifest`
- Result: all three formal resource workloads passed with the candidate within the relative PSS,
  stack, CPU, anonymous-memory, TLS, and HTTP/2 contracts.
- Candidate local checks: `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, and
  `cargo test control_plane_mesh --lib` passed; the Mesh suite reported `70 passed`.
- Candidate full Rust checks: `cargo test` reported `1517 passed`, `7 passed` in the main binary,
  all non-ignored integration tests passed, and the repository-contract ignored tests remained
  ignored.
