# Mesh Resource Evidence

- Candidate commit: `f1a81bcb960a32e69ee931c7647a1d40f8cfac86`
- Evidence persistence: this documentation follow-up only records the immutable testbox run below;
  the runtime source and generated Web shell are bound by the candidate commit and archive hashes.
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260923_030959_f1a81bcb960a_5a6ca840`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@6491df193f015db6ef8e384e376f72d7890d5b21`
- Source archive SHA-256: `ac31afc18b2b8525838889c86645cee8438a5f932d381ab1b70fffa20ad4f927`
- Generated Web archive SHA-256: `bda19552af45e9e567d50564db8235497e46786c40b0ad92f89485171fed9069`
- Baseline archive SHA-256: `eaf35858e8e657269bf8454fbb92b21f869b5bb0a0e11dffae166f1b12cd721e`
- 50-peer baseline/candidate peak PSS: `32311/31260 KiB`
- 50-peer baseline/candidate anonymous PSS: `16024/14496 KiB` (both below the 18 MiB limit)
- 50-peer baseline/candidate CPU ticks: `170/180`
- 50-peer TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per peer
- Repository summary peak PSS: `28747 KiB`
- Source journal peak PSS: `26587 KiB`; CPU p95: `0%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Cargo cache: candidate and baseline source slots were refreshed for the new candidate; candidate,
  baseline, and resource-test build phases completed in `368s`, `397s`, and `434s` respectively.
- Result: all three formal resource workloads passed with candidate PSS strictly below `32MiB`.
