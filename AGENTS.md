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
- Managed runtime memory defaults are part of every deployment path: Xray uses
  `GOMEMLIMIT=16MiB`, `GOGC=50`, and level-0 `bufferSize=0`; cloudflared uses
  `GOMEMLIMIT=12MiB`, `GOGC=50`, and `TUNNEL_MANAGEMENT_DIAGNOSTICS=false`.
  Release assets use the pinned low-memory Go build, and upgrade backfill must
  preserve operator overrides.
- Managed VLESS HTTPS canary certificates use Cloudflare DNS-01. Propagation checks query
  Cloudflare and Google over DoH; supported nodes require outbound HTTPS to those resolvers, but
  do not require direct authority access on UDP/TCP port 53.
- Managed-default VLESS/REALITY endpoints also provide the control-plane Mesh ingress: signed
  `health-v2` and `mesh-v2` traffic is routed by the canary only to fixed local XP loopback paths;
  public `/generate_204` and authority-based camouflage remain separate. `XP_MESH_PROXY_URL` is
  public-fallback egress compatibility, not a Mesh tunnel. Multi-node internal-auth v2 upgrades
  require a one-shot maintenance marker: host-managed nodes bootstrap from a verified target
  `xp-ops` binary using `upgrade --allow-internal-auth-v2-cutover`, while containers use the target
  image's `container mark-internal-auth-v2-cutover` command. Web upgrade must return
  `coordinated_upgrade_required` until the durable epoch has been established; once consumed, v1
  rollback is unsupported.
- Managed-default endpoint ports become cluster-owned after creation or auto-adoption.
  `XP_DEFAULT_VLESS_PORT` and `XP_DEFAULT_SS_PORT` are bootstrap inputs only; normal `xp` startup,
  `xp-ops xp sync-node-meta`, container restart, and upgrade must preserve the port stored in Raft.
  Existing ports are intentionally changed or endpoints deleted through the Admin UI/API. Keeping
  a bootstrap env value after an explicit deletion recreates the endpoint on the next reconcile
  when no same-kind endpoint remains available for conservative adoption.
- Web-triggered automatic upgrade is supported only for host-managed `systemd` / `OpenRC` nodes via
  the restricted `xp-ops _upgrade-runner` one-shot delegation installed by `xp-ops init`. systemd
  nodes must include the root-owned fixed `/usr/local/libexec/xp-upgrade-trigger` helper and narrow
  `/etc/sudoers.d/91-xp-upgrade` policy; the polkit rule is only a compatibility supplement because
  CentOS 7-class polkit does not reliably expose `unit` / `verb` details. OpenRC nodes must include
  the root-owned fixed `/usr/local/libexec/xp-openrc-upgrade-trigger` helper and narrow
  `/etc/doas.conf` rules for its `--check` probe and fixed `xp-upgrade start`; the unprivileged
  service verifies readiness through the helper and must not read root-only `doas.conf` directly.
  This helper cannot be installed by the already-running pre-helper `xp-ops` binary: when an
  existing OpenRC host crosses this release boundary, the root operator must run `xp-ops init`
  after `xp-ops upgrade` completes. Missing or removed helper assets remain unsupported until that
  explicit reinitialization; do not make readiness checks start the one-shot service as a fallback.
  Docker / Compose nodes
  must keep using host-side image / Compose replacement and must be documented as Web-upgrade
  unsupported. The systemd `xp-upgrade.service` must invoke `xp-ops _upgrade-runner` directly and
  pass `XP_DATA_DIR` through the unit environment, not through shell-expanded `--data-dir` command
  text. If the one-shot runner fails before writing a terminal status, the admin upgrade status API
  must reconcile the durable `running` / `restarting` status to `failed`.
- The Web client must treat an unstructured start 5xx or a restart-boundary network interruption as
  an unknown result, not a terminal failure: maintain a same-tab 60-second status observation with
  2.5-second polling, preserve only the remaining window through refresh, and end only on a
  terminal status, a structured rejection, or timeout. During that observation, duplicate Upgrade
  remains disabled even if the popover is manually closed; timeout stays locked until a manual
  status query proves an active job (new window) or idle/terminal state (unlock).
- Host-managed `systemd` deployments with provider NAT / DDNS / Tunnel in front of the node are first-class supported environments.
- A host-managed upgrade must complete the locked `xp` and managed runtime phase before
  replacing `xp-ops`; an `xp-ops` self-update must never be allowed to skip that service phase.
- A successful service restart requires the selected systemd or OpenRC manager to report the
  service ready after restart; a zero exit status from an asynchronous restart command is not enough.
- Docker Compose deployments using the official single-image runtime are first-class supported environments.
- Cloudflare Tunnel provisioning preserves shared-Tunnel configuration outside the XP hostname.
  It reuses the existing single `cloudflared` process and validates before an atomic replacement.
  Moving an XP hostname automatically runs ownership preflight and rollback.
  A legacy Tunnel with additional hostnames is rejected before writes because one cloudflared
  process cannot keep both Tunnel connectors alive.
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
