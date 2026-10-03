# Local Candidate Checks

- Runtime candidate commit: `7712c651ccd6e4253f137438d4d9cc648dcc2805`
- Base commit: `ed109323`
- `cargo fmt --all -- --check`: passed
- `cargo clippy -- -D warnings`: passed
- `bun run check:style-budget`: passed
- `git diff --check`: passed
- `cargo test control_plane_mesh --lib`: `88 passed`
- `cargo test http::mesh::preflight --lib`: `6 passed`
- `cargo test`: `1539` library tests passed, `7` main tests passed, all non-ignored integration
  tests and doc tests passed, and ignored shared-resource tests remained ignored by repository
  policy.
- Web `bun run lint`: passed; `bun run typecheck`: passed; Vitest: `105` files and `507` tests
  passed on the immediately preceding runtime candidate; this candidate has no Web file changes.
- The runtime commits' pre-commit and commit-msg hooks passed Rust Clippy, rustfmt, style budget,
  Markdown formatting, and commitlint.
