# Local Candidate Checks

- Candidate commit: `d0fe824625d5e58fca8455b26d5d3032bf9a45fb`
- `cargo fmt --all -- --check`: passed
- `cargo clippy --lib -- -D warnings`: passed
- `bun run check:style-budget`: passed
- `git diff --check`: passed before the evidence-only documentation update
- `cargo test control_plane_mesh --lib`: `78 passed`
- `cargo test`: `1526` library tests passed, `7` main tests passed, all non-ignored integration
  tests passed, and ignored shared-resource tests remained ignored by repository policy.
- Web `lint`, `typecheck`, and Vitest passed on the unchanged Web tree before the candidate
  rebuild; the shared-testbox runner rebuilt the generated Web shell from this candidate.
- The repository-wide all-target Clippy run remains subject to pre-existing warnings outside this
  change in `src/http/resource_monitoring.rs`,
  `src/history_repository/replica/runtime/retention_tests.rs`, and
  `src/history_storage/diagnostics.rs`. The production-library Clippy gate passed without warnings.
- The implementation commit's pre-commit hooks passed Rust Clippy, rustfmt, style budget, and
  commitlint checks.
