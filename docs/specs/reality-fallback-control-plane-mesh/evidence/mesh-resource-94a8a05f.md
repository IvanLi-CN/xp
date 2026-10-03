# Mesh Resource Evidence

- Candidate commit: `94a8a05f7ef46791bb4c8e57b394a80a3bcf17c1`
- Evidence persistence: this record binds the current candidate source and generated Web shell to
  the immutable shared-testbox run below.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260929_142242_94a8a05f7ef4_835d4f37`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@ed109323`
- Source archive SHA-256: `43605c6e11034c2378057b0e58e564cf085b1e3f5653092a71ab3f5fd2ba478e`
- Generated Web archive SHA-256: `0e657de1216ef2787f9d17b9baea8a8f9bd04420a48b8f1bb83b67ad23382e70`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- 50-peer baseline/candidate peak PSS: `33407/32456 KiB`
- 50-peer baseline/candidate anonymous PSS: `17228/16724 KiB`
- 50-peer baseline/candidate stack PSS: `43139/42188 KiB`
- 50-peer baseline/candidate CPU ticks: `172/183`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `29509 KiB`
- Source journal peak PSS: `29112 KiB`; CPU p95: `1%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke reached all 50 peers before the 15-second sample; formal workload also observed
  50 persistent H2 connections.
- Cargo build phases completed in `381s` (candidate), `382s` (baseline), and `384s` (resource test).
- Evidence manifest:
  `/tmp/xp-testbox-evidence-94a8a05f/20260929_142242_94a8a05f7ef4_835d4f37.manifest`
- Result: all three formal resource workloads passed with candidate total PSS below `32MiB`.
