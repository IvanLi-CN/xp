# Mesh Resource Evidence

- Candidate commit: `5a9855f66a3ddb1d5a8bad11c8d3880c6326f657`
- Baseline commit: `ed109323103627f98363705f43f610c6fc093e61`
- Testbox run: `20261002_041331_5a9855f66a3d_be2de413`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds per candidate and baseline
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323`
  `scripts/testbox/run-shared-mesh-resource-e2e.sh`
- Source archive SHA-256: `6ee3d325ed18392e2af328dcf57e4aadb10a69b279177d2b26be7b61162dea82`
- Generated Web archive SHA-256: `73361b062240f647c862ba148c3b61666285bfc2f35a97ccac53689ddcb36599`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- 50-peer baseline/candidate total PSS: `32963/32095 KiB`
- 50-peer baseline/candidate anonymous PSS: `16460/16660 KiB`
- 50-peer baseline/candidate file-backed PSS: `16811/17018 KiB`
- 50-peer baseline/candidate stack PSS: `62363/51004 KiB`
- 50-peer baseline/candidate CPU ticks: `201/206` (candidate below the `211` tick limit)
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer; every peer observed at least 18 requests
- Repository summary peak PSS: `28216 KiB`
- Source journal peak PSS: `27910 KiB`; CPU p95: `1.00%`; additional read bytes: `0`;
  max RSS delta: `0 bytes`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke and formal workload reached all 50 peers with one persistent H2 connection each.
- Cargo build phases completed in `355s` (candidate), `360s` (baseline), and `450s` (resource test).
- Evidence manifest:
  `/tmp/xp-testbox-evidence-5a9855f6/20261002_041331_5a9855f66a3d_be2de413.manifest`
- Result: remote resource test returned `3 passed; 0 failed; 0 ignored` in `1853.45s`; the
  formal resource gate passed within the relative PSS, stack, CPU, anonymous-memory, TLS, and
  HTTP/2 contracts. Exact remote cleanup left no run directory, Compose container, or Compose
  network.
- A1 mapping: authoritative apply and unchanged reconcile writer-queue regressions passed in the
  local candidate checks.
- A2 mapping: queued-writer admission timeout, remaining-budget Public fallback, and pre-dispatch
  classification regressions passed in the local candidate checks.
- A3 mapping: EOF, error, dropped, unpolled, deadline, signed-response no-retry, stream-lease, and
  shared completion-worker regressions passed in the local candidate checks.
- A4 mapping: this run records the passed 50-peer H2/resource workloads; it does not instrument
  committed/`last_applied` convergence or election-timeout counters, so those remain covered by
  the deterministic state-machine/admission regressions rather than being claimed as runtime
  measurements.
- A5 mapping: the same candidate SHA has separate local full-check evidence recorded alongside
  the final delivery state.
