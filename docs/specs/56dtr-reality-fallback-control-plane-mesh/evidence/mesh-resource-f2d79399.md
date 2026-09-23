# Mesh Resource Evidence

- Candidate commit: `6748acc7be6da703c6c41469ced2e390d87973b2`
- Evidence persistence: this documentation follow-up only records the immutable testbox run below;
  the runtime source and generated Web shell are bound by the candidate commit and archive hashes.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260923_052046_6748acc7be6d_8a85574d`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@6491df193f015db6ef8e384e376f72d7890d5b21`
- Source archive SHA-256: `88af0e882a8dde98d2d9498442c134398bf75880941bbac3f11c7b90f10fc974`
- Generated Web archive SHA-256: `379cafe361fb37d6243289bc15b76f9f0cba5b869ba28d2c939c273581f46819`
- Baseline archive SHA-256: `eaf35858e8e657269bf8454fbb92b21f869b5bb0a0e11dffae166f1b12cd721e`
- 50-peer baseline/candidate peak PSS: `31820/32727 KiB`
- 50-peer baseline/candidate anonymous PSS: `15484/15836 KiB` (both below the 18 MiB limit)
- 50-peer baseline/candidate CPU ticks: `177/186`
- 50-peer TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per peer
- Repository summary peak PSS: `28846 KiB`
- Source journal peak PSS: `26495 KiB`; CPU p95: `1%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Cargo cache: candidate and baseline source slots were refreshed for the new candidate; candidate,
  baseline, and resource-test build phases completed in `408s`, `447s`, and `479s` respectively.
- Result: all three formal resource workloads passed with candidate PSS strictly below `32MiB`.
