# Mesh Resource Evidence

- Candidate commit: `807e2d7b5c575d9230c64110c27d2dec17292546`
- Evidence persistence: this documentation follow-up only records the immutable testbox run below;
  the runtime source and generated Web shell are bound by the candidate commit and archive hashes.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260923_021043_807e2d7b5c57_727ae71e`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@4ed26f58daef74705064282c828a82bc9d75f564`
- Source archive SHA-256: `9e000051692f6d11b545dc6547868d8c6532523534e059fd528329cf18e58233`
- Generated Web archive SHA-256: `8f689d8ba0c54d21504dfc1ae97c203415b1eaeab8347eca9e91e7658f8d4dd5`
- Baseline archive SHA-256: `eaf35858e8e657269bf8454fbb92b21f869b5bb0a0e11dffae166f1b12cd721e`
- 50-peer baseline/candidate peak PSS: `32425/32046 KiB`
- 50-peer baseline/candidate anonymous PSS: `15404/14948 KiB` (both below the 18 MiB limit)
- 50-peer baseline/candidate CPU ticks: `173/182`
- 50-peer TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per peer
- Repository summary peak PSS: `28288 KiB`
- Source journal peak PSS: `27032 KiB`; CPU p95: `0%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Cargo cache: candidate and baseline source slots were hits; candidate, baseline, and resource-test
  build phases each completed in `1s`.
- Result: all three formal resource workloads passed with candidate PSS strictly below `32MiB`.
