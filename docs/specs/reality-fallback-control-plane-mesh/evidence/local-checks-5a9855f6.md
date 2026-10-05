# Mesh Local Checks

- Candidate source SHA: `5a9855f66a3ddb1d5a8bad11c8d3880c6326f657`
- `cargo test control_plane_mesh --lib`: `93 passed; 0 failed`
- `cargo test`: `1543` library tests passed, `7` main tests passed, all non-ignored integration
  and doc tests passed. The three resource workloads and other environment-dependent suites
  remained ignored in this local run.
- `cargo clippy -- -D warnings`: passed
- `cargo fmt --all -- --check`: passed
- `python3 scripts/check-style-budget.py`: passed
- `bunx --no-install dprint check` for the owning SPEC, IMPLEMENTATION, and current evidence:
  passed
- `git diff --check`: passed
- The formal shared-testbox resource result for this same source SHA is recorded in
  `./mesh-resource-5a9855f6.md`.
