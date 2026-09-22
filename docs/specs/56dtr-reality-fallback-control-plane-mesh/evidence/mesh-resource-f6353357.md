# Mesh Resource Evidence

- Candidate commit: `f63533578d0f`
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_SUMMARY_ONLY=1 scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260922_042917_f63533578d0f_96a11c4f`
- Environment: actual release XP process, Linux `MemoryMax=128MiB`, `MemorySwapMax=0`
- Source journal peak PSS: `26115 KiB`
- Repository summary peak PSS: `28137 KiB`
- Result: both workloads passed with PSS strictly below `32MiB`; journal capacity guard
  remained active at the bounded backlog.
