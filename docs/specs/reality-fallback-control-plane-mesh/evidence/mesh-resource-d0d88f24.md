# Mesh Resource Evidence

- Candidate commit: `d0d88f242c1ec85c2d41bd6d1a010a40e1055266`
- Baseline commit: `ed109323`
- Testbox run: `20260930_193626_d0d88f242c1e_af73dce8`
- Environment: actual release XP process and Xray, Linux `MemoryMax=128MiB`,
  `MemorySwapMax=0`, 50-peer workload for 900 seconds per candidate and baseline
- Runner: `XP_RUN_MESH_RESOURCE=1 XP_E2E_ONLY_MESH_RESOURCE=1`
  `XP_MESH_RESOURCE_BASELINE_SHA=ed109323 scripts/testbox/run-shared-quota-xray-e2e.sh`
- Source archive SHA-256: `b8f80b4c8d5b11a3626c742be233c7ad06a385b6e39be6fea1435f33573d05a1`
- Generated Web archive SHA-256: `14cd2d5caf93d788cc0a6a40dd32de60556187aef1b7a07c0ca0ad0fa5c02b9d`
- Baseline archive SHA-256: `36fc1c62ca113d8fefaeae9eee1769174169d0bed524f0599e90e01d68a6899f`
- Candidate/baseline total PSS: `32235/31408 KiB`
- Candidate/baseline anonymous PSS: `17932/17224 KiB`
- Candidate/baseline stack PSS: `40811/59229 KiB`
- Candidate/baseline CPU ticks: `189/213`
- Candidate TLS accepts: `50`; non-H2 requests: `0`; active and peak active connections: `1` per
  peer
- Repository summary peak PSS: `27336 KiB`
- Source journal peak PSS: `27059 KiB`; CPU p95: `1%`; additional read bytes: `0`; max RSS
  delta: `8192 bytes`
- Source journal state remained `journal_capacity_guard` at `19971` pending segments.
- Candidate smoke and formal workload reached all 50 peers with one persistent H2 connection each.
- Result: all three formal resource workloads passed with the candidate within the relative PSS,
  stack, CPU, anonymous-memory, TLS, and HTTP/2 contracts.
- A1 mapping: `src/raft/storage/file/tests.rs::authoritative_normal_apply_does_not_queue_`
  `mesh_gate_writer` and `src/reconcile/tests/reverse_gate_tests.rs::unchanged_authoritative_`
  `reconcile_does_not_queue_gate_writer` passed.
- A2 mapping: `src/control_plane_mesh/mesh_gate_tests.rs::mesh_admission_timeout_keeps_public_`
  `fallback_available_behind_a_queued_writer`,
  `src/control_plane_mesh/cleanup_tests.rs::public_admission_timeout_does_not_dispatch_`, and
  `::direct_mesh_admission_timeout_preserves_pre_dispatch_classification` passed.
- A3 mapping: finite, EOF, error, dropped, unpolled, and deadline body tests in
  `src/control_plane_mesh/mesh_gate_tests.rs`, plus
  `src/raft/network_http/transport_reuse_tests.rs::signed_mesh_body_holds_gate_until_deadline_`
  `without_public_retry`, passed.
- A4 mapping: this manifest and its output log record the passed 50-peer, repository-summary, and
  source-journal workloads; voter membership and production data were not touched.
- A5 mapping: `./local-checks-d0d88f24.md` records the candidate SHA, exact local commands, and
  observed counts; this file records the same-SHA shared-testbox gate and archive hashes.
- Evidence manifest:
  `/tmp/xp-testbox-evidence-d0d88f24-retry1/20260930_193626_d0d88f242c1e_af73dce8.manifest`
- Evidence log:
  `/tmp/xp-testbox-evidence-d0d88f24-retry1/20260930_193626_d0d88f242c1e_af73dce8.log`
