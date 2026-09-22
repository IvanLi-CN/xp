# Mesh Resource Evidence

- Candidate commit: `816eaad5e278372b1af6498570bbb4e3c0569084`
- Evidence persistence: this documentation follow-up only records the immutable testbox run below;
  the runtime source and generated Web shell are bound by the candidate commit and archive hashes.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260922_103722_816eaad5e278_5d8e2be3`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@4ed26f58daef74705064282c828a82bc9d75f564`
- Source archive SHA-256: `15fdb01e26800e1b37623112f1a75827a98c87a8187ce5669ab7c801f60b64da`
- Generated Web archive SHA-256: `c0f99b56f65cea83d75c5a7ab94b72b18f0caf1a4c0c34dc93cc0e640cd48a07`
- 50-peer baseline/candidate peak PSS: `31677/32040 KiB`
- 50-peer baseline/candidate anonymous PSS: `15048/14924 KiB` (both below the 18 MiB limit)
- 50-peer baseline/candidate CPU ticks: `168/164`
- 50-peer TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per peer
- Repository summary peak PSS: `28803 KiB`
- Source journal peak PSS: `27088 KiB`; CPU p95: `1%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Result: all three resource workloads passed with candidate PSS strictly below `32MiB`.
