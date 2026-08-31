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
- Host-managed XP-generated systemd/OpenRC Xray services may opt into the root-only
  `xp-ops ingress-guard` admission guard. It owns only `table inet xp_ingress_guard`,
  matches initial non-loopback TCP SYNs by the current Xray cgroup v2, and does not
  enumerate ports or add a resident root process. Docker/Compose, custom Xray assets,
  and unsupported kernels remain out of scope. `enable`, `observe`, `set-limits`, and
  `disable` require root plus `--yes`; no xp/Web/API/polkit/sudo/doas firewall
  delegation may be added. Enforced service preparation failure must keep Xray stopped.
- Managed VLESS HTTPS canary certificates use Cloudflare DNS-01. Propagation checks query
  Cloudflare and Google over DoH; supported nodes require outbound HTTPS to those resolvers, but
  do not require direct authority access on UDP/TCP port 53.
- Managed-default VLESS/REALITY endpoints also provide the control-plane Mesh ingress: signed
  `health-v2` and `mesh-v2` traffic is routed by the canary only to fixed local XP loopback paths;
  public `/generate_204` and authority-based camouflage remain separate. All outbound Mesh users
  share one process-wide HTTP/2-only client with one idle connection per origin and a 120-second
  idle bound; public direct and dynamic relay use separate clients. An internal-auth v2 cluster
  upgrade requires a one-shot maintenance marker: host-managed nodes bootstrap from a verified
  target `xp-ops` binary using `upgrade --allow-internal-auth-v2-cutover`, while containers use the
  target image's `container mark-internal-auth-v2-cutover` command. Web upgrade must return
  `coordinated_upgrade_required` until the durable epoch has been established; once consumed, v1
  rollback is unsupported.
- Cluster history repositories persist their replica state in `${XP_DATA_DIR}/history.sqlite3` on
  each configured repository node. Membership, lifecycle and capacity are Raft-backed; repository
  sync uses Reality Mesh and Cloudflare Tunnel/public origin as equal direct paths, then the
  Raft-assigned Reality Mesh Reverse relay, and only then the in-memory encrypted dynamic relay.
  Reverse uses XP-owned loopback `127.0.0.1:10086` with authenticated TCP-only SOCKS and does
  not add a public listener. No static Mesh proxy environment or compatibility path exists.
- Reality Mesh Reverse is an additive control-plane path. It uses Raft assignments, an XP-owned
  `127.0.0.1:10086` TCP-only SOCKS portal, and upstream Xray dynamic APIs; it never adds a public
  listener. A durable assignment does not itself keep a target Xray initiating outbound installed:
  each local `(epoch,target,rendezvous,role,generation)` Link must acquire signed health within a
  10-second probe or maintain a 120-second lease, otherwise its outbound is removed and retry is
  bounded locally. `XP_REVERSE_MESH_ENABLED=false` is the supported node-local fail-closed
  rollback and leaves Raft assignments, Direct/Public, and membership intact. Fresh joins may
  carry a short-lived public-only `reverse_mesh_bootstrap` marker, but learner catch-up and
  log-index promotion remain authoritative. If container-side Xray restart recovery cannot
  complete after tombstone overflow, Reverse stays disabled until an operator restarts the
  container; Direct/Public and membership remain available.
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
  `/etc/doas.conf` rules for its `--check` probe and fixed `start` action; the helper may only
  inspect, zap a crashed `xp-upgrade`, and start that fixed service. The unprivileged service
  verifies readiness through the helper and must not read root-only `doas.conf` directly.
  This helper cannot be installed by the already-running pre-helper `xp-ops` binary: when an
  existing OpenRC host crosses this release boundary, the root operator must run `xp-ops init`
  after `xp-ops upgrade` completes. Missing or removed helper assets remain unsupported until that
  explicit reinitialization; do not make readiness checks start the one-shot service as a fallback.
  Docker / Compose nodes
  must keep using host-side image / Compose replacement and must be documented as Web-upgrade
  unsupported. The systemd `xp-upgrade.service` must invoke `xp-ops _upgrade-runner` directly and
  pass `XP_DATA_DIR` through the unit environment, not through shell-expanded `--data-dir` command
  text. If the one-shot runner fails before writing a terminal status, the admin upgrade status API
  must reconcile the durable `running` / `restarting` status to `failed`. The Web start guard uses
  an advisory process lock rather than lock-file existence and releases it before invoking the host
  trigger. OpenRC backgrounds the one-shot and zaps its service state after every runner exit.
- All host-managed upgrade paths share one disk-retention contract: transaction-local binary
  backups only, zero `.bak.*`/`.failed.*` binaries after success or successful rollback; a
  `rollback_failed` filesystem error preserves the affected transaction backup for manual recovery
  and rejects a later upgrade until that backup has been recovered;
  no capacity-tiered offline fallback. Before any download or replacement, managed stale artifacts
  and the exact `/tmp/xp-ops` workspace are cleaned without following symlinks, then both write
  filesystems must have at least `128 MiB` free. Existing `/var/backups/xp`, configuration,
  credentials, certificates, Raft/WAL, and unknown files are out of scope. The latest failure may
  retain only bounded `${XP_DATA_DIR}/upgrade/diagnostics.json`; success removes it.
- The Web client must treat an unstructured start 5xx or a restart-boundary network interruption as
  an unknown result, not a terminal failure: maintain a same-tab 60-second status observation with
  2.5-second polling, preserve only the remaining window through refresh, and end only on a
  terminal status, a structured rejection, or timeout. During that observation, duplicate Upgrade
  remains disabled even if the popover is manually closed; timeout stays locked until a manual
  status query proves an active job (new window) or idle/terminal state (unlock).
  `upgrade_already_running` remains observable only when the immediate status refresh finds an
  active job. An older idle/terminal status is an explicit stale conflict: report it immediately
  and unlock Upgrade instead of waiting for the observation timeout.
- The Web PWA must keep each build's complete static app shell in a build-versioned precache. New
  workers wait for explicit user confirmation; `xp_sw_metadata` owns cross-tab
  `clientId -> buildId` records. Old app-shell caches remain until reconciliation proves that no
  controlled client uses them.
  The only exception is a one-time Workbox legacy migration: after a complete precache, an exact
  same-scope `workbox-precache-v2-<scope>` plus no valid XP owner after a bounded 1-second probe may
  `skipWaiting()` in the background. It must not `clients.claim()` or refresh open pages.
  It may only later remove recorded legacy and orphan XP app-shell caches after all live
  clients declare valid ownership.
  Navigation and static subresources must never be assembled from mixed builds.
- Web/API compatibility is independent of PWA build IDs. The supported window is the immutable
  3.22/3.21/3.20 release inventory; Web probes additive capabilities first, then a strict stable
  release tag and local endpoint fingerprints. Missing profile capabilities degrade only their
  feature, while declared capability 404/schema failures remain regressions.
- Host-managed `systemd` deployments with provider NAT / DDNS / Tunnel in front of the node are first-class supported environments.
- An owner-approved private Docker follower may omit the managed-default VLESS/REALITY endpoint.
  It remains a voter and must expose its registered private `api_base_url` to the serving voters
  for signed lifecycle-capability verification; this exception never permits skipping that voter
  or falling back from a configured Mesh endpoint.
- Host-managed initial joins provision Tunnel/DNS without starting `cloudflared`, then join the
  cluster and write `/etc/xp/xp.env` before enabling then starting or restarting `xray`, `xp`,
  and optional `cloudflared` in order with readiness checks. A joined node is successful only
  after its public `api_base_url/health` returns HTTP `200`; post-join health failure preserves
  membership and metadata for retry. Geo remains disabled by default and is written only by explicit
  host-managed `--ip-geo` / TUI opt-in, never by automatic backfill.
- Raft member roles are only `voter`, transient `learner`, and `absent`; “non-voter member” is a
  set description, never a promotable role. Every voter maps to one DesiredState Node. An unexpected
  learner or orphan voter blocks lifecycle writes and is reported, never automatically promoted,
  deleted, rolled back, or repaired by periodic work.
- Fresh joins use a durable two-stage protocol: the leader records a Join membership operation and
  reservation, registers the learner, and returns bootstrap identity before catch-up; after the
  authenticated runtime starts, the leader coordinator waits for the recorded log index and promotes
  only that recorded learner. Same-request retries replay the reservation without extending its
  10-minute activation deadline. Expired sessions are cleaned by the leader; existing-node recovery
  remains separate.
- Fresh join, restore and delete require every current voter to expose
  `cluster.membership-lifecycle-v1`. Orphan-voter repair first proves one exact non-leader orphan,
  then requires that capability only from every retained DesiredState-mapped voter; the proven
  orphan is excluded only from its own repair preflight. Retained voters use signed Mesh capability
  reads, with the predecessor's legacy public capability endpoint only after the signed capability
  probe receives that predecessor's `404` from the missing new route. Other missing or invalid
  acknowledgements remain terminal. Operators must finish the XP rolling upgrade one voter at a
  time while maintaining quorum before lifecycle writes resume. The pristine learner accepts only
  signed internal-auth v2
  Raft traffic, but any elected leader in the authenticated cluster may complete its initial
  replication after failover.
- A host-managed upgrade must complete the locked `xp` and managed runtime phase before
  replacing `xp-ops`; an `xp-ops` self-update must never be allowed to skip that service phase.
- A successful service restart requires the selected systemd or OpenRC manager to report the
  service ready after restart; OpenRC must report ready twice successively. A zero exit status from
  an asynchronous restart command is not enough.
- Docker Compose deployments using the official single-image runtime are first-class supported environments.
- Cloudflare Tunnel provisioning preserves shared-Tunnel configuration outside the XP hostname.
  It reuses the existing single `cloudflared` process and validates before an atomic replacement.
  Moving an XP hostname automatically runs ownership preflight and rollback.
  A freshly created remote-config Tunnel may return Cloudflare `1055` before any configuration
  exists; XP treats that exact response as an empty remote configuration and writes the owned
  ingress. Other configuration failures remain terminal.
  A matching persisted XP Tunnel with a credentials payload naming that Tunnel ID is reused by
  non-interactive deploy; it is not treated as a name collision that can generate a suffixed
  Tunnel name.
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
- Mihomo mirror private targets are authorized per node by `XP_MIHOMO_ALLOWED_PRIVATE_CIDRS` and the node-local `${XP_DATA_DIR}/mihomo-resource-policy.json` Web override; policy state is never stored in Raft.
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
