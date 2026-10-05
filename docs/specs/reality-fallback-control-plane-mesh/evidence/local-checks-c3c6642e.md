# Local Candidate Checks

- Runtime candidate commit: `c3c6642ec41b8679dfcfed4b5d7fad4f462b888a`
- Follow-up test-only commit: `5bf4ab387853e5f5d056160c7190beba1bbae0c7`
- `cargo fmt --all -- --check`: passed
- `cargo clippy --lib -- -D warnings`: passed
- `cargo clippy -- -D warnings`: passed
- `bun run check:style-budget`: passed
- `git diff --check`: passed
- `cargo test control_plane_mesh --lib`: `82 passed`
- `cargo test`: `1530` library tests passed, `7` main tests passed, all non-ignored integration
  tests passed, and ignored shared-resource tests remained ignored by repository policy.
- Web `bun run lint`: passed; `bun run typecheck`: passed; Vitest: `105` files and `507` tests
  passed. The existing jsdom canvas `getContext()` notices were non-fatal test output.
- The runtime and test commits' pre-commit hooks passed Rust Clippy, rustfmt, style budget, and
  commitlint.
