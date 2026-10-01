# Mesh Resource Evidence

- Runtime candidate commit: `96050406e978c3b713d66efb573a7059127c8782`
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
- The prior resource manifests are not evidence for this candidate because this repair changes
  Mesh failure cleanup and health-preflight state handling.
- A4 remains unchecked until the exact runtime candidate and baseline are run on the isolated
  shared testbox and produce a manifest.
