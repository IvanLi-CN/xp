# Mesh Local Checks

- Candidate source SHA: `392f4b2a9fedf10bbb7bdf52cf4ef369118b5ceb`
- `cargo test control_plane_mesh --lib`: `95 passed; 0 failed`
- `cargo test mesh_telemetry --lib`: `26 passed; 0 failed`
- `cargo test`: `1547` library tests passed, `7` main tests passed, and all non-ignored
  integration and doc tests passed. Environment-dependent workloads remained ignored in the
  local run.
- `cargo clippy -- -D warnings`: passed
- `cargo fmt --all -- --check`: passed
- `python3 scripts/check-style-budget.py`: passed
- `git diff --check`: passed
- The formal shared-testbox resource result for this same source SHA is recorded in
  `./mesh-resource-392f4b2a.md`.
