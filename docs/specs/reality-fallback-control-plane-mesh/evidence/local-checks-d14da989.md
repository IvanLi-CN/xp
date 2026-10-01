# Local Candidate Checks

- Candidate commit: `d14da9899cd8b215e632d555db078c6f9a33340a`
- `cargo fmt --all -- --check`: passed
- `cargo clippy --lib -- -D warnings`: passed
- `cargo clippy -- -D warnings`: passed
- `bun run check:style-budget`: passed
- `git diff --check`: passed
- `cargo test control_plane_mesh --lib`: `81 passed`
- `cargo test`: `1529` library tests passed, `7` main tests passed, all non-ignored integration
  tests passed, and ignored shared-resource tests remained ignored by repository policy.
- Web `bun run lint`: passed; `bun run typecheck`: passed; Vitest: `105` files and `507` tests
  passed. The existing jsdom canvas `getContext()` notices were non-fatal test output.
- The implementation commit's pre-commit hooks passed Rust Clippy, rustfmt, style budget, and
  commitlint.
