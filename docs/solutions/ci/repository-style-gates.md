# Repository style gates must be executable and budget-backed

## Problem

Formatter and lint configuration can drift into a state where checks exist but do not enforce the
intended contract. Examples include disabled formatters, tools that only work after dependency
installation, hooks that are configured but not installed, and no guardrail against very large
files.

## Guardrail

Keep three layers active together:

- tool-native checks, such as `cargo fmt --check`, `cargo clippy`, Biome, TypeScript, tests, and
  dprint;
- a repository-specific style-budget check for limits that the native tools do not own;
- local hooks and CI jobs that execute the same policy.

The style-budget checker should exclude generated files, dependency directories, lockfiles, binary
assets, and build outputs. It should fail on new violations rather than printing advisory-only
reports.

For repositories with existing historical violations, use a checked-in baseline. Store only the
current metrics for files that exceed the hard threshold, and fail if those files get worse. New
files should have no baseline entry and must satisfy the hard threshold immediately.

## Module boundary rule

When a file exceeds the size budget, split it by ownership and behavior. Good boundaries include
schema versus migrations, HTTP DTOs versus route handlers, renderer stages versus reference
rewrites, and component state versus presentational subcomponents.

Do not create `part1`, `part2`, or thin wrapper files whose only purpose is lowering a line count.
That makes navigation worse and turns the checker into noise. Baseline entries should shrink when
real ownership boundaries are extracted.

## Verification

- Run the style-budget checker locally and in CI.
- Run hook commands through `lefthook` after installation, not only by reading `lefthook.yml`.
- Keep project docs updated with the commands developers are expected to run.
