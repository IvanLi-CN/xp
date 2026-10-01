# Mesh Resource Evidence

- Runtime candidate commit: `7712c651ccd6e4253f137438d4d9cc648dcc2805`
- Baseline commit: `ed109323`
- Required runner:
  `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323`
  `scripts/testbox/run-shared-mesh-resource-e2e.sh`
- Status: interrupted during the exact-candidate run; candidate build passed, but the baseline
  build produced no progress after `Compiling xp` and the runner was stopped after the shared
  testbox SSH cleanup path also stopped accepting new banner connections. A4 is not passed.
- Testbox: `codex-testbox` / `192.168.31.15`
- Run manifest: `20261001_133215_7712c651ccd6_0806f2bf` (candidate SHA matched the runtime
  candidate; local manifest status remained `running` because the interrupted cleanup could not
  reconnect to the testbox).
- Candidate build: passed in `383s`; baseline build: no terminal result before interruption.
- The prior non-current `f933bd92` run passed the isolated workload: baseline XP total/anon PSS
  `32752/17868 KiB`, candidate `32506/16488 KiB`, stack peaks `42688/42442 KiB`, 50 TLS accepts,
  zero non-H2 requests, repository summary peak `28990 KiB`, and source journal peak `27903 KiB`.
  It is retained as historical evidence and does not satisfy same-SHA A4.
- The earlier `96050406` run completed without OOM or process restart but exceeded the total-PSS
  relative ceiling by `197 KiB`; it remains a failed historical attempt.
- A4 remains unchecked until this exact runtime candidate and baseline are run on the isolated
  shared testbox and produce a passing manifest.
