# Mesh Resource Evidence

- Candidate commit: `bdf84f066757517ca51a1c9a6db077001daae0a5`
- Evidence persistence: this record binds the candidate source and generated Web shell to the
  immutable shared-testbox run below.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260930_100241_bdf84f066757_20cbb054`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@ed109323`
- Source archive SHA-256: `b5992fe0a31b18b7988ef784d5b10ca0d30e6db543d6330d84661d451f55b190`
- Generated Web archive SHA-256: `dc37ead5fddc76d3f42361589c88c5c95cb2f69f09327c3e51e66a15e534af3f`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- 50-peer baseline/candidate peak PSS: `34067/34118 KiB`
- 50-peer baseline/candidate anonymous PSS: `17028/16668 KiB`
- 50-peer baseline/candidate stack PSS: `64069/57793 KiB`
- 50-peer baseline/candidate CPU ticks: `228/221`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `29365 KiB`
- Source journal peak PSS: `29237 KiB`; CPU p95: `0%`; additional read bytes: `0`; max RSS
  delta: `8192 bytes`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke reached all 50 peers before the formal sample; formal workload also observed 50
  persistent H2 connections.
- Cargo build phases completed in `2s` (candidate), `0s` (baseline), and `1s` (resource test).
- Evidence manifest:
  `/tmp/xp-testbox-evidence-bdf84f06-rerun/20260930_100241_bdf84f066757_20cbb054.manifest`
- Result: all three formal resource workloads passed with the candidate within the relative PSS,
  stack, CPU, anonymous-memory, TLS, and HTTP/2 contracts.
