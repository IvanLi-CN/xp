# Mesh Resource Evidence

- Candidate commit: `1e54d319f92d0d6f3db90d27fa0be52cb04f5d67`
- Baseline commit: `ed109323103627f98363705f43f610c6fc093e61`
- Testbox run: `20260930_232126_1e54d319f92d_e95cfc7e`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds per candidate and baseline
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323 scripts/testbox/run-shared-quota-xray-e2e.sh`
- Source archive SHA-256: `8baabac1ae9a68bbdc694013e520bd6bf3a738604810abe3c1842a530c141fc1`
- Generated Web archive SHA-256: `90dd46f73268b47e6e08fc5cdaee4854e090271be1c4bb88815f3cb979a26b80`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- Candidate/baseline total PSS: `33824/33454 KiB` (`+370 KiB`)
- Candidate/baseline anonymous PSS: `16592/16904 KiB`
- Candidate/baseline file-backed PSS: `17155/16900 KiB`
- Candidate/baseline stack PSS: `48596/64638 KiB`
- Candidate/baseline CPU ticks: `169/172`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `29283 KiB`
- Source journal peak PSS: `29087 KiB`; CPU p95: `1%`; additional read bytes: `0`; max RSS
  delta: `16384 bytes`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke and formal workload reached all 50 peers with one persistent H2 connection each.
- Result: `3 passed; 0 failed; 0 ignored` in `1853.48s`; the formal resource gate passed within
  the relative PSS, stack, CPU, anonymous-memory, TLS, and HTTP/2 contracts.
- A1 mapping: `src/raft/storage/file/tests.rs::authoritative_normal_apply_does_not_queue_`
  `mesh_gate_writer` and `src/reconcile/tests/reverse_gate_tests.rs::unchanged_authoritative_`
  `reconcile_does_not_queue_gate_writer` passed.
- A2 mapping: `src/control_plane_mesh/mesh_gate_tests.rs::mesh_admission_timeout_keeps_public_`
  `fallback_available_behind_a_queued_writer`, `src/control_plane_mesh/cleanup_tests.rs::public_`
  `admission_timeout_does_not_dispatch_`, and `::direct_mesh_admission_timeout_preserves_pre_`
  `dispatch_classification` passed.
- A3 mapping: finite, EOF, error, dropped, unpolled, and deadline body tests in
  `src/control_plane_mesh/mesh_gate_tests.rs`, plus
  `src/raft/network_http/transport_reuse_tests.rs::signed_mesh_body_holds_gate_until_deadline_`
  `without_public_retry`, passed.
- A4 mapping: this manifest records the passed 50-peer H2/resource workloads; the separate
  host-managed fresh-join evidence records membership and service-recovery checks. Neither run
  claims uninstrumented committed/`last_applied` convergence or election-timeout counters.
- A5 mapping: `./local-checks-1e54d319.md` records the same candidate SHA and local checks.
- Evidence manifest:
  `/tmp/xp-testbox-evidence-1e54d319/20260930_232126_1e54d319f92d_e95cfc7e.manifest`
- Evidence log:
  `/tmp/xp-testbox-evidence-1e54d319/20260930_232126_1e54d319f92d_e95cfc7e.log`
