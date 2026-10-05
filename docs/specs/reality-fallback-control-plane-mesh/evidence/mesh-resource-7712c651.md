# Mesh Resource Evidence

- Runtime candidate commit: `7712c651ccd6e4253f137438d4d9cc648dcc2805`
- Baseline commit: `ed109323`
- Required runner:
  `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323`
  `scripts/testbox/run-shared-mesh-resource-e2e.sh`
- Status: failed in two exact-candidate runs; the shared testbox completed both workload windows,
  but the CPU relative budget failed. A4 is not passed.
- Testbox: `codex-testbox` / `192.168.31.15`
- Retry manifest: `20261001_153236_7712c651ccd6_876724b6` completed with `status=failed`.
  Baseline CPU was `153` ticks and candidate CPU was `162` ticks; the calculated 5% limit was
  `161` ticks. TLS accepts (`50`), non-H2 requests (`0`), one active connection per peer, and
  the summary/journal resource tests passed.
- Second retry manifest: `20261001_162646_7712c651ccd6_2cb4549b` completed with
  `status=failed`. Baseline CPU was `154` ticks and candidate CPU was `185` ticks; the calculated
  5% limit was `162` ticks. The same connection/protocol checks and summary/journal resource
  tests passed.
- The prior non-current `f933bd92` run passed the isolated workload: baseline XP total/anon PSS
  `32752/17868 KiB`, candidate `32506/16488 KiB`, stack peaks `42688/42442 KiB`, 50 TLS accepts,
  zero non-H2 requests, repository summary peak `28990 KiB`, and source journal peak `27903 KiB`.
  It is retained as historical evidence and does not satisfy same-SHA A4.
- The earlier `96050406` run completed without OOM or process restart but exceeded the total-PSS
  relative ceiling by `197 KiB`; it remains a failed historical attempt.
- A4 remains unchecked until this exact runtime candidate and baseline are run on the isolated
  shared testbox and produce a passing manifest.
