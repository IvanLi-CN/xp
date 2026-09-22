# Mesh Resource Evidence

- Candidate commit: `f2d793995ee865e7dfa975bc2713ce63113cb643`
- Release head: `c5bb5c24` (evidence-only persistence commit; runtime source and generated Web
  shell are unchanged from the tested candidate commit above)
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Testbox run: `20260922_080410_f2d793995ee8_e0832620`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds
- Baseline commit: `origin/main@4ed26f58daef74705064282c828a82bc9d75f564`
- Source archive SHA-256: `c699781f89bcd22f149e0d634bd6de01e41bde0c0bd9850a60e18fcce727695c`
- Generated Web archive SHA-256: `95c80eae36fbcbaad0d22338a20bab85a1f56083dedd15b780216f28f2c34e44`
- 50-peer baseline/candidate peak PSS: `31556/32199 KiB`
- 50-peer baseline/candidate anonymous PSS: `15400/15200 KiB` (both below the 18 MiB limit)
- 50-peer baseline/candidate CPU ticks: `164/168`
- 50-peer TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per peer
- Repository summary peak PSS: `28810 KiB`
- Source journal peak PSS: `28143 KiB`; CPU p95: `0%`; additional read bytes: `0`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Result: all three resource workloads passed with candidate PSS strictly below `32MiB`.
