# Mesh Resource Evidence

- Candidate commit: `6273ac4e3895ab658b8721a771ce87da7c38a427`
- Evidence persistence: this documentation follow-up only records the immutable testbox run below;
  the runtime source and generated Web shell are bound by the candidate commit and archive hashes.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260923_041902_6273ac4e3895_dc10f30f`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@6491df193f015db6ef8e384e376f72d7890d5b21`
- Source archive SHA-256: `619b0be19300dc95341289a451c9f54b0a749d19a0c5e020c1c23b79acb26fde`
- Generated Web archive SHA-256: `089c9112bfff81ba276fa4d4f6af28b066ea189da7e8ca9b8f002f7a49817adc`
- Baseline archive SHA-256: `eaf35858e8e657269bf8454fbb92b21f869b5bb0a0e11dffae166f1b12cd721e`
- 50-peer baseline/candidate peak PSS: `31758/30567 KiB`
- 50-peer baseline/candidate anonymous PSS: `15516/14760 KiB` (both below the 18 MiB limit)
- 50-peer baseline/candidate CPU ticks: `172/188`
- 50-peer TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per peer
- Repository summary peak PSS: `27870 KiB`
- Source journal peak PSS: `27919 KiB`; CPU p95: `1%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Cargo cache: candidate and baseline source slots were refreshed for the new candidate; candidate,
  baseline, and resource-test build phases completed in `357s`, `364s`, and `403s` respectively.
- Result: all three formal resource workloads passed with candidate PSS strictly below `32MiB`.
