# Mesh Resource Evidence

- Runtime candidate commit: `c3c6642e`
- Baseline commit: `ed109323`
- Required runner:
  `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323`
  `scripts/testbox/run-shared-mesh-resource-e2e.sh`
- Status: pending fresh run after the testbox restart and the response-telemetry fix.
- Testbox: `codex-testbox` / `192.168.31.15`
- The earlier run for `96050406` completed without OOM or process restart and passed the
  connection/H2 and summary/journal workloads, but failed the total-PSS relative ceiling:
  candidate `34780 KiB` versus baseline `33559 KiB` (`+1221 KiB`, limit `+1024 KiB`).
- A4 remains unchecked until the exact runtime candidate and baseline are run on the isolated
  shared testbox and produce a passing manifest.
