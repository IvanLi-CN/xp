# Mesh Resource Evidence

- Candidate commit: `819899fa4cefdca3d35a1ae480572bc77a4d1a3b`
- Evidence persistence: this record binds the current candidate source and generated Web shell to
  the immutable shared-testbox run below.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260930_050825_819899fa4cef_cd955c61`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@ed109323`
- Source archive SHA-256: `22dfe01ce2942c856c147579e366912ff1709a04cb28a3ae8ccdb22a156210e4`
- Generated Web archive SHA-256: `05000e12b4c0d1592afb4ed7278f31a8ba151f358d277cf3bd46ea87b7933732`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- 50-peer baseline/candidate peak PSS: `34683/33949 KiB`
- 50-peer baseline/candidate anonymous PSS: `16872/16548 KiB`
- 50-peer baseline/candidate stack PSS: `64203/63469 KiB`
- 50-peer baseline/candidate CPU ticks: `178/176`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `28905 KiB`
- Source journal peak PSS: `28313 KiB`; CPU p95: `0%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke reached all 50 peers before the 15-second sample; formal workload also observed
  50 persistent H2 connections.
- Cargo build phases completed in `1s` (candidate), `1s` (baseline), and `0s` (resource test).
- Evidence manifest:
  `/tmp/xp-testbox-evidence-819899fa-rerun/20260930_050825_819899fa4cef_cd955c61.manifest`
- Result: all three formal resource workloads passed with the candidate within the relative PSS,
  stack, CPU, anonymous-memory, TLS, and HTTP/2 contracts.
