# Mesh Resource Evidence

- Candidate commit: `59fa7d6fa65381d3bf34d074d562f4eaad8831c2`
- Baseline commit: `ed109323`
- Testbox run: `20260930_172342_59fa7d6fa653_ebde60ba`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds per candidate and baseline
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323 scripts/testbox/run-shared-quota-xray-e2e.sh`
- Source archive SHA-256: `fb2268086064d5382850eb853e15cdb12fc66cb9ad4d7dccba42f2125851f3ac`
- Generated Web archive SHA-256: `ae7004acd82f3f51d135cd44dbe5ccdb0999ed6663c80d291aa7252332fa206b`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- Candidate/baseline total PSS: `32887/33859 KiB`
- Candidate/baseline anonymous PSS: `16112/17544 KiB`
- Candidate/baseline stack PSS: `47947/62158 KiB`
- Candidate/baseline CPU ticks: `189/196`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `28486 KiB`
- Source journal peak PSS: `28647 KiB`; CPU p95: `1%`; additional read bytes: `0`; max RSS
  delta: `118784 bytes`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke and formal workload reached all 50 peers with one persistent H2 connection each.
- Result: all three formal resource workloads passed with the candidate within the relative PSS,
  stack, CPU, anonymous-memory, TLS, and HTTP/2 contracts.
- A1 mapping: `src/raft/storage/file/tests.rs::authoritative_normal_apply_does_not_queue_`
  `mesh_gate_writer` passed; `src/reconcile/tests/reverse_gate_tests.rs::unchanged_authoritative_`
  `reconcile_does_not_queue_gate_writer` covers the generation-advance fast path.
- A2 mapping: `src/control_plane_mesh/mesh_gate_tests.rs::mesh_admission_timeout_keeps_public_`
  `fallback_available_behind_a_queued_writer` and `::direct_mesh_admission_timeout_preserves_`
  `pre_dispatch_classification` passed.
- A3 mapping: finite, EOF, error, dropped, unpolled, and deadline body tests in
  `src/control_plane_mesh/mesh_gate_tests.rs`, plus signed protocol isolation and new epoch/public
  telemetry regressions in `src/control_plane_mesh/gate.rs`, passed.
- Candidate local checks: `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, and
  `cargo test control_plane_mesh --lib` passed; the Mesh suite reported `72 passed`.
- Candidate full Rust checks: `cargo test` reported `1519 passed` in lib tests, `7 passed` in the
  main binary, all non-ignored integration tests passed, and repository-contract ignored tests
  remained ignored.
- Evidence manifest: `/tmp/xp-testbox-evidence-59fa7d6f-retry2/`
  `20260930_172342_59fa7d6fa653_ebde60ba.manifest`
