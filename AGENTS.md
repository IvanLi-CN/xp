# Repository Guidelines

## Project Structure & Module Organization

- `src/`: Rust control-plane service (`xp`) and core logic (e.g., cycle calculation).
- `web/`: Vite + React admin UI (TanStack Router/Query, Tailwind + DaisyUI).
- `docs/desgin/`: design specs (requirements, architecture, API, quota, cluster, workflows).
- `docs/plan/`: milestone-level plan and acceptance criteria.
- `scripts/`: repo tooling and helper scripts.
- Build artifacts: `target/`, `web/dist/` (generated; don’t edit).

## Build, Test, and Development Commands

- Install repo tooling (commitlint + dprint): `npm install`
- Install Git hooks (required): `lefthook install`
- Run backend locally: `cargo run` (default bind `127.0.0.1:8080`)
- Backend checks: `cargo test`, `cargo fmt`, `cargo clippy -- -D warnings`
- Install frontend deps: `cd web && npm install`
- Run frontend dev server: `cd web && npm run dev` (binds `127.0.0.1:60080`)
- Frontend checks: `cd web && npm run lint`, `cd web && npm run typecheck`, `cd web && npm test`
- UI regression: `cd web && npm run storybook`, `cd web && npm run test-storybook`
- E2E: `cd web && npm run test:e2e` (Playwright)

## Coding Style & Naming Conventions

- Rust: format with rustfmt (`cargo fmt`); keep Clippy clean (warnings are errors).
- TypeScript/React: Biome handles formatting + linting (`web/` scripts `format` / `lint`).
- Markdown: formatted via `npx dprint fmt` (see `dprint.json`).
- Naming: Rust modules/functions `snake_case`, types `CamelCase`; React components `PascalCase`.

## Testing Guidelines

- Rust unit tests live next to code (e.g., `src/cycle.rs`); prefer pure functions where possible.
- Web unit tests use Vitest; use `*.test.ts(x)` or `web/tests/` for higher-level tests.
- Keep unit tests deterministic; reserve Playwright E2E for critical user flows.

## Commit & Pull Request Guidelines

- Commits follow Conventional Commits (types enforced by commitlint), e.g. `docs: update plan`.
- Commit subject/body must be English-only; subject must start lowercase; header ≤72 chars.
- Prefer a short commit body explaining “why” for non-trivial changes.
- PRs: include a summary, testing notes (commands run), and screenshots for UI changes; update `docs/` when behavior changes.
