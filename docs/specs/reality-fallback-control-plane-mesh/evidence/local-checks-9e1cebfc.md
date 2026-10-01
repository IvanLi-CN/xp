# Local Candidate Checks

- Runtime candidate commit: `9e1cebfc2c100a59eb85745c7ac0399dd69b1246`
- Base commit: `ed109323103627f98363705f43f610c6fc093e61`
- `cargo fmt --all -- --check`: passed
- `cargo clippy -- -D warnings`: passed
- `bun run check:style-budget`: passed
- `git diff --check`: passed
- `cargo test control_plane_mesh --lib`: `90 passed`
- `cargo test`: `1540` library tests passed, `7` main tests passed, all non-ignored integration
  and doc tests passed; ignored shared-resource tests remained ignored by repository policy.
- Web lint, typecheck, and Vitest evidence is unchanged from the preceding candidate because
  this repair contains no Web changes.
- The signed commit hook passed Rust Clippy, rustfmt, style budget, and commitlint.
