# Mesh Resource Evidence

- Candidate commit: `ad485653b2ab8e1db3b8e2dc680a0a94e45abf58`
- Evidence persistence: this record binds the current candidate source and generated Web shell to
  the immutable shared-testbox run below.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260930_063234_ad485653b2ab_b012b686`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@ed109323`
- Source archive SHA-256: `c3ce80d5896c9d836da5b6cb635f32bbd4720ecd7cba60794bf4c35d5ea8e0b7`
- Generated Web archive SHA-256: `769d61b953440be1b55c64ab68f32543b3092b74428e3b111b68b4b976fc05ef`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- 50-peer baseline/candidate peak PSS: `34024/32660 KiB`
- 50-peer baseline/candidate anonymous PSS: `17388/16112 KiB`
- 50-peer baseline/candidate stack PSS: `60111/53506 KiB`
- 50-peer baseline/candidate CPU ticks: `221/176`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `29001 KiB`
- Source journal peak PSS: `28256 KiB`; CPU p95: `1%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke reached all 50 peers before the 15-second sample; formal workload also observed
  50 persistent H2 connections.
- Cargo build phases completed in `428s` (candidate), `456s` (baseline), and `440s` (resource test).
- Evidence manifest:
  `/tmp/xp-testbox-evidence-ad485653/20260930_063234_ad485653b2ab_b012b686.manifest`
- Result: all three formal resource workloads passed with the candidate within the relative PSS,
  stack, CPU, anonymous-memory, TLS, and HTTP/2 contracts.
