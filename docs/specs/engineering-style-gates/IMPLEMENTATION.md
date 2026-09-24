# Implementation

## Current Status

- Style-budget checker added under `scripts/`.
- CI and hook integration are part of the implementation scope.
- Rust formatting is expected to be enabled for the whole repository.
- Large modules are decomposed by domain boundary, not by arbitrary line count.
- Inline Rust test modules have been extracted where they formed a real boundary.
- Remaining historical files above the hard thresholds are tracked in
  `scripts/style-budget-baseline.json` as explicit non-growth budgets until each can be reduced by
  focused ownership refactors.

## Verification

Validated locally on 2026-06-28:

- `bun install --frozen-lockfile`
- `cd web && bun install --frozen-lockfile`
- `cd web && bun run lint`
- `cd web && bun run typecheck`
- `cd web && bun run test`
- `cd web && bun run build`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `bunx --no-install dprint check`
- `python3 scripts/check-style-budget.py`
- `git diff --check`
- `lefthook install`
- `lefthook run pre-commit`
- commitlint fixture check via `bunx --no-install commitlint --edit`

`lefthook run pre-commit` was also exercised before staging; it correctly skipped file-scoped
commands when no staged files existed. The commit path must exercise the same hook after staging,
where the changed Rust, Markdown, and script files are visible to the hook runner.
