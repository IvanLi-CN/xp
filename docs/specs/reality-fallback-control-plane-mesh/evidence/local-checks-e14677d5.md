# Local Candidate Checks

- Runtime candidate commit: `e14677d5d95c9276878d9d49d3251f347926fe2f`
- Base commit: `ed109323`
- `cargo fmt --all -- --check`: passed
- `cargo clippy -- -D warnings`: passed
- `bun run check:style-budget`: passed
- `git diff --check`: passed
- `cargo test control_plane_mesh --lib`: `85 passed`
- `cargo test`: `1533` library tests passed, `7` main tests passed, all non-ignored integration
  tests and doc tests passed, and ignored shared-resource tests remained ignored by repository
  policy.
- Web `bun run lint`: passed; `bun run typecheck`: passed; Vitest: `105` files and `507` tests
  passed on the immediately preceding runtime candidate; this candidate has no Web file changes.
- The runtime commit pre-commit and commit-msg hooks passed Rust Clippy, rustfmt, style budget,
  and commitlint.
