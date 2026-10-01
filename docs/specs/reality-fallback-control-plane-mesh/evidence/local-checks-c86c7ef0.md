# Local Candidate Checks

- Runtime candidate commit: `c86c7ef0116155bb70e17562c7343024978f86ff`
- Base commit: `ed109323`
- `cargo fmt --all -- --check`: passed
- `cargo clippy -- -D warnings`: passed
- `bun run check:style-budget`: passed
- `git diff --check`: passed
- `cargo test control_plane_mesh --lib`: `85 passed`
- `cargo test`: `1532` library tests passed, `7` main tests passed, all non-ignored integration
  tests passed, and ignored shared-resource tests remained ignored by repository policy.
- Web `bun run lint`: passed; `bun run typecheck`: passed; Vitest: `105` files and `507` tests
  passed. The existing jsdom canvas `getContext()` notices were non-fatal test output.
- The runtime commit pre-commit and commit-msg hooks passed Rust Clippy, rustfmt, style budget,
  and commitlint.
