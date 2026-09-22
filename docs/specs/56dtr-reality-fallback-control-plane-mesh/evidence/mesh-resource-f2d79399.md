# Mesh Resource Evidence

- Candidate commit: `85808c23e4f372ae3a5b016750be9f6c8a22636c`
- Evidence persistence: this documentation follow-up only records the immutable testbox run below;
  the runtime source and generated Web shell are bound by the candidate commit and archive hashes.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260922_115944_85808c23e4f3_af2b89cc`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@4ed26f58daef74705064282c828a82bc9d75f564`
- Source archive SHA-256: `ff09dbf8285166d680ee1bb3cbd27b2dc82f21d9d162c7ae3ae14bea8e90e9b1`
- Generated Web archive SHA-256: `b1f72b5f40a8a3c2929db13aafe42f5561534c26e4e72477d9b75a06ed1daf93`
- 50-peer baseline/candidate peak PSS: `31756/30774 KiB`
- 50-peer baseline/candidate anonymous PSS: `15164/14568 KiB` (both below the 18 MiB limit)
- 50-peer baseline/candidate CPU ticks: `155/168`
- 50-peer TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per peer
- Repository summary peak PSS: `26307 KiB`
- Source journal peak PSS: `26094 KiB`; CPU p95: `0%`; additional read bytes: `16384`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Result: all three resource workloads passed with candidate PSS strictly below `32MiB`.
