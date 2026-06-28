# Repository Guidelines

## Project Structure & Module Organization

- `src/`: Rust cluster manager service (`xp`) and core logic (e.g., cycle calculation).
- `web/`: Vite + React admin UI (TanStack Router/Query, Tailwind + DaisyUI).
- DaisyUI theme prompt: `https://daisyui.com/llms.txt give me a light daisyUI 5 theme with tropical color palette`
- `docs/desgin/`: design specs (requirements, architecture, API, quota, cluster, workflows).
- `docs/plan/`: milestone-level plan and acceptance criteria.
- `scripts/`: repo tooling and helper scripts.
- Build artifacts: `target/`, `web/dist/` (generated; don’t edit).

## Deployment Contract

- Owner-facing deployment truth lives in `docs/ops/README.md` and the active `docs/specs/**/SPEC.md` files, not in ad hoc chat decisions.
- The project must keep these deployment environments first-class:
- Host-managed `xp + xray + cloudflared` on `systemd` Linux nodes.
- Host-managed `xp + xray + cloudflared` on `OpenRC` Linux nodes.
- Single-image Docker / Compose nodes driven by `xp-ops container run`.
- Host-managed upgrades must not be treated as a compatibility afterthought for container-only features. If a managed-default VLESS / Mihomo relay / canary behavior is shipped, the expected host-managed upgrade path must be explicit and tested.
- Host-managed `systemd` deployments with provider NAT / DDNS / Tunnel in front of the node are first-class supported environments.
- Docker Compose deployments using the official single-image runtime are first-class supported environments.
- If an environment is only partially supported or blocked by current implementation limits, the limitation must be stated concretely in specs and ops docs together with the required operator intervention.
- When deployment or upgrade behavior changes, update `AGENTS.md`, `docs/ops/**`, and the owning spec together so the supported-environment matrix stays aligned.

## Build, Test, and Development Commands

- Install repo tooling (commitlint + dprint): `bun install`
- Install Git hooks (required): `lefthook install`
- Style budget: `bun run check:style-budget`
- Run backend locally: `cargo run` (default bind `127.0.0.1:62416`)
- Sanitize Mihomo subscriptions/configs before sharing: `xp-ops mihomo redact [SOURCE]` (`SOURCE` supports URL, file path, `-` for stdin, or omit for stdin)
- Backend checks: `cargo test`, `cargo fmt`, `cargo clippy -- -D warnings`
- Install frontend deps: `cd web && bun install`
- Run frontend dev server: `cd web && bun run dev` (binds `127.0.0.1:60080`)
- Frontend checks: `cd web && bun run lint`, `cd web && bun run typecheck`, `cd web && bun run test`
- UI regression: `cd web && bun run storybook`, `cd web && bun run test-storybook`
- E2E: `cd web && bun run test:e2e` (Playwright)

## Coding Style & Naming Conventions

- Rust: format with rustfmt (`cargo fmt`); keep Clippy clean (warnings are errors).
- TypeScript/React: Biome handles formatting + linting (`web/` scripts `format` / `lint`).
- Markdown: formatted via `bunx --no-install dprint fmt` (see `dprint.json`).
- Line and source-file budgets are enforced by `scripts/check-style-budget.py`.
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
