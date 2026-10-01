# Mesh Resource Evidence

- Candidate commit: `9e1cebfc2c100a59eb85745c7ac0399dd69b1246`
- Baseline commit: `ed109323103627f98363705f43f610c6fc093e61`
- Testbox run: `20261001_185135_9e1cebfc2c10_355dc51e`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds per candidate and baseline
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323 scripts/testbox/run-shared-mesh-resource-e2e.sh`
- Source archive SHA-256: `01a09d08eebdfe0c74f630497176894b890829c9e2e74245612f426c7bf510e5`
- Generated Web archive SHA-256: `f6a3ff93fa659b4d052f78350347f8d5a0f6a650dc8b1bb50b1b478a8f155a4a`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- Candidate/baseline total PSS: `32079/35209 KiB` (`-3130 KiB`)
- Candidate/baseline anonymous PSS: `16900/16644 KiB`
- Candidate/baseline file-backed PSS: `18597/18479 KiB`
- Candidate/baseline stack PSS: `61483/64705 KiB`
- Candidate/baseline CPU ticks: `189/200` (`-11` ticks)
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer; every peer observed at least 18 requests.
- Repository summary peak PSS: `28909 KiB`
- Source journal peak PSS: `29014 KiB`; CPU p95: `0.00%`; additional read bytes: `0`; max RSS
  delta: `65536 bytes`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke and formal workload reached all 50 peers with one persistent H2 connection each.
- Result: remote resource test returned `3 passed; 0 failed; 0 ignored` in `1853.08s`; the formal
  resource gate passed within the relative PSS, stack, CPU, anonymous-memory, TLS, and HTTP/2
  contracts. The main runner exited successfully and exact remote cleanup left no run directory,
  Compose container, or Compose network.
- Evidence wrapper note: the local manifest
  `/tmp/xp-testbox-evidence-9e1.retry2/20261001_185135_9e1cebfc2c10_355dc51e.manifest` says
  `status=failed` because the evidence directory was created after the runner's `tee` opened its
  log path. The remote test process and main runner exit status are the authoritative workload
  result; no workload assertion failed.
- A1 mapping: authoritative apply and unchanged reconcile writer-queue regressions passed in the
  local candidate checks.
- A2 mapping: queued-writer admission timeout, remaining-budget Public fallback, and pre-dispatch
  classification regressions passed in the local candidate checks.
- A3 mapping: EOF, error, dropped, unpolled, deadline, signed-response no-retry, and shared
  completion-worker regressions passed in the local candidate checks.
- A4 mapping: this run records the passed 50-peer H2/resource workloads; it does not instrument
  committed/`last_applied` convergence or election-timeout counters, so those remain covered by
  the deterministic state-machine/admission regressions rather than being claimed as runtime
  measurements.
- A5 mapping: `./local-checks-9e1cebfc.md` records the same candidate SHA and local checks.
