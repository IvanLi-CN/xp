# Mesh Resource Evidence

- Candidate commit: `5ad0685c6bfaccb66b5bdd09240f91df4e84874d`
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260922_053203_5ad0685c6bfa_999245ed`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- 50-peer baseline/candidate peak PSS: `32651/30756 KiB`
- 50-peer baseline/candidate CPU ticks: `152/163`
- 50-peer TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per peer
- Repository summary peak PSS: `28719 KiB`
- Source journal peak PSS: `27352 KiB`; CPU p95: `1%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Result: all three resource workloads passed with candidate PSS strictly below `32MiB`.
