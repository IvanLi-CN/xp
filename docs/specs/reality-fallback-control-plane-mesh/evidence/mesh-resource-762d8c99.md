# Mesh Resource Evidence

- Candidate commit: `762d8c99c3c9056388186253a10b46442572d5d5`
- Evidence persistence: this record binds the current candidate source and generated Web shell to
  the immutable shared-testbox run below.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260929_132545_762d8c99c3c9_08c54fae`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@ed109323`
- Source archive SHA-256: `d5977380a2f4a0b4b32a45be49e00329de1be24310d9b232ccbb375dfe45be56`
- Generated Web archive SHA-256: `d9f3a4974103d961d2346e7b28e2bd2a7be22e16b25936720586ec9acf27ea2c`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- 50-peer baseline/candidate peak PSS: `34263/30820 KiB`
- 50-peer baseline/candidate anonymous PSS: `18956/16120 KiB`
- 50-peer baseline/candidate stack PSS: `44391/40920 KiB`
- 50-peer baseline/candidate CPU ticks: `191/178`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `28489 KiB`
- Source journal peak PSS: `28138 KiB`; CPU p95: `1%`; additional read bytes: `94208`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke reached all 50 peers before the 15-second sample; formal workload also observed
  50 persistent H2 connections.
- Cargo build phases completed in `381s` (candidate), `398s` (baseline), and `467s` (resource test).
- Evidence manifest:
  `/tmp/xp-testbox-evidence-762d8c99/20260929_132545_762d8c99c3c9_08c54fae.manifest`
- Result: all three formal resource workloads passed with candidate total PSS below `32MiB`.
