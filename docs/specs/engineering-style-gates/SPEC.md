# Engineering Style Gates

## Background

The repository already had CI, hooks, formatters, and linters, but several gates could pass
without enforcing the intended style contract. Rust formatting was disabled, local hook
installation was not proven, and no repository-level check prevented very long source files or
lines from returning.

## Goals

- Make formatting, linting, and hook checks executable and CI-backed.
- Enforce readable line and source-file budgets for code and project Markdown.
- Split oversized modules only along real domain boundaries.
- Keep the style policy visible in project docs and reusable solution notes.

## Non-goals

- Do not change public HTTP APIs, persistent data schemas, subscription output semantics, or UI
  behavior.
- Do not split files mechanically by line count, numeric suffixes, or arbitrary chunks.
- Do not bypass hooks, formatters, lint, typecheck, tests, or commit verification.

## Requirements

- `cargo fmt --check` MUST use real rustfmt formatting.
- `cd web && bun run lint` MUST use the lockfile-installed Biome version.
- A repository style-budget checker MUST fail on:
  - checked source or Markdown lines longer than 100 characters;
  - Rust, TypeScript, TSX, JavaScript, or CSS files longer than 1000 lines.
- Generated outputs, dependency directories, lockfiles, binary assets, and build artifacts MUST be
  excluded from the style-budget checker.
- Oversized Rust and Web files MUST be decomposed by module responsibility while preserving the
  previous public behavior.
- Existing files that still exceed the hard thresholds MUST be listed in a committed baseline with
  their current metrics. Baseline entries are debt, not permanent exemptions: the checker must fail
  if they grow worse, and each future touch should reduce or remove the baseline entry through real
  module-boundary extraction.

## Acceptance

- `bun install --frozen-lockfile`
- `cd web && bun install --frozen-lockfile && bun run lint && bun run typecheck && bun run test`
- `cd web && bun run build`
- `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- `bunx --no-install dprint check`
- `python3 scripts/check-style-budget.py`
- `lefthook install` creates working `pre-commit` and `commit-msg` hooks.

## Documentation

- README and AGENTS must mention the style-budget gate with the existing local development
  commands.
- The reusable CI/style-gate lesson must live under `docs/solutions/ci/`.
