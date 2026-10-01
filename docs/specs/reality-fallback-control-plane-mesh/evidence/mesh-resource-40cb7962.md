# Mesh Resource Evidence

- Runtime candidate commit: `40cb79620bafbfe4f568b17b08e4cc78f2d5ba08`
- Baseline commit: `ed109323`
- Required runner:
  `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323`
  `scripts/testbox/run-shared-mesh-resource-e2e.sh`
- Status: pending final-candidate run after the shared testbox restart and the operation-ordering
  and half-open body-lifecycle fixes.
- Testbox: `codex-testbox` / `192.168.31.15`
- The prior non-current `f933bd92` run passed the isolated workload: baseline XP total/anon PSS
  `32752/17868 KiB`, candidate `32506/16488 KiB`, stack peaks `42688/42442 KiB`, 50 TLS accepts,
  zero non-H2 requests, repository summary peak `28990 KiB`, and source journal peak `27903 KiB`.
  It is retained as historical evidence and does not satisfy same-SHA A4.
- The earlier `96050406` run completed without OOM or process restart but exceeded the total-PSS
  relative ceiling by `197 KiB`; it remains a failed historical attempt.
- A4 remains unchecked until this exact runtime candidate and baseline are run on the isolated
  shared testbox and produce a passing manifest.
