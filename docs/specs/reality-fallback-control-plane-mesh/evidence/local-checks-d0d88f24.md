# Local Candidate Checks

- Candidate commit: `d0d88f242c1ec85c2d41bd6d1a010a40e1055266`
- `cargo fmt --all -- --check`: passed
- `cargo clippy -- -D warnings`: passed
- `bun run check:style-budget`: passed
- `cargo test control_plane_mesh --lib`: `76 passed`
- `cargo test transport_reuse_tests --lib`: `7 passed`
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test`: `1524` library tests passed,
  `7` main tests passed, all non-ignored integration tests passed, and ignored shared-resource
  tests remained ignored by repository policy.
- The full run completed on the candidate SHA after the final runtime changes; the shared-testbox
  resource gate is the empirical evidence for the ignored multi-node workloads.
