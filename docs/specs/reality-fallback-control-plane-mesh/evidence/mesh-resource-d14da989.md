# Mesh Resource Evidence

- Candidate commit: `d14da9899cd8b215e632d555db078c6f9a33340a`
- Baseline commit: `ed109323`
- Required runner:
  `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323`
  `scripts/testbox/run-shared-mesh-resource-e2e.sh`
- Status: blocked before the runner could start.
- Testbox: `codex-testbox` / `192.168.31.15`
- Observed on 2026-10-01: TCP port 22 was reachable, but both 10-second and 30-second
  `ssh -o BatchMode=yes -o ConnectionAttempts=1` attempts ended with
  `Connection timed out during banner exchange`.
- No remote files, containers, services, or production nodes were modified by these attempts.
- The prior `d0fe8246` resource manifest is not evidence for this candidate because this repair
  changes the half-open probe admission and success-telemetry wait paths.
- A4 remains unchecked until the exact candidate and baseline are run on the isolated shared
  testbox and produce a manifest.
