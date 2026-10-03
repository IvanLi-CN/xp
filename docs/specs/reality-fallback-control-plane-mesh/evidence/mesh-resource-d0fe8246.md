# Mesh Resource Evidence

- Candidate commit: `d0fe824625d5e58fca8455b26d5d3032bf9a45fb`
- Baseline commit: `ed109323`
- Testbox run: `20261001_051427_d0fe824625d5_3f820cf9`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds per candidate and baseline
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323`
  `scripts/testbox/run-shared-quota-xray-e2e.sh`
- Source archive SHA-256: `c3f1fed0d60aa7758792bf5a05156b293173099d3b8451f5640e020fea627c89`
- Generated Web archive SHA-256: `b99d8576f462fe4508563f8a3474c531144883a19fb25d404404bd1c0be08291`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- Candidate/baseline total PSS: `32955/32898 KiB`
- Candidate/baseline anonymous PSS: `16524/16948 KiB`
- Candidate/baseline stack PSS: `43063/42957 KiB`
- Candidate/baseline CPU ticks: `190/197`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `29100 KiB`
- Source journal peak PSS: `28368 KiB`; CPU p95: `1%`; additional read bytes: `0`; max RSS
  delta: `126976 bytes`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke and formal workload reached all 50 peers with one persistent H2 connection each.
- Result: all three formal resource workloads passed with the candidate within the relative PSS,
  stack, CPU, anonymous-memory, TLS, and HTTP/2 contracts. The runner exited `0` after `1852.14s`.
- A1 mapping: authoritative state-machine apply and unchanged authoritative reconcile fast-path
  regressions passed in the existing Raft/reconcile test seams.
- A2 mapping: queued-writer admission timeout, pre-dispatch classification, Public fallback, and
  half-open release regressions passed in `src/control_plane_mesh/mesh_gate_tests.rs` and
  `src/control_plane_mesh/cleanup_tests.rs`.
- A3 mapping: finite, EOF, error, dropped, unpolled, and deadline body tests passed, including
  signed-header timeout without Public retry.
- A4 mapping: this manifest and output log record the passed 50-peer, repository-summary, and
  source-journal workloads; voter membership and production data were not touched.
- Evidence manifest:
  `/tmp/xp-testbox-evidence-d0fe8246/20261001_051427_d0fe824625d5_3f820cf9.manifest`
- Evidence log: `/tmp/xp-testbox-evidence-d0fe8246/20261001_051427_d0fe824625d5_3f820cf9.log`
