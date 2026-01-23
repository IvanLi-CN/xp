# xp

`xp` is a **multi-host Xray cluster manager** for self-hosted deployments. It connects multiple
machines into one cluster and provides a single place to manage endpoints, users, quotas, and
subscriptions.

Each host runs two processes: `xp` and `xray`. `xp` stores cluster-wide desired state with Raft
(OpenRaft) and reconciles the local `xray` runtime via the Xray gRPC API.

This repository also includes an admin UI (Vite + React). The built assets (`web/dist`) are embedded
into the `xp` binary at build time and served as a SPA by default.

## Features

- Dynamic Xray inbound + client management (VLESS + REALITY, Shadowsocks 2022)
- Subscription output: Raw URI / Base64 / Clash YAML (`GET /api/sub/{subscription_token}`; see `docs/desgin/subscription.md`)
- Quotas: cycle windows, bidirectional traffic, auto-ban, optional auto-unban (see `docs/desgin/quota.md` and `XP_QUOTA_*`)
- Cluster consistency: 1–20 nodes Raft (OpenRaft); write requests are serialized by the leader
- Embedded admin UI: served by `xp` (default: `http://127.0.0.1:62416/`)
- Ops tool: `xp-ops` (install/init services, self-upgrade, `xp` upgrade, optional Cloudflare Tunnel)

## Docs

- Design: `docs/desgin/README.md`
- Operations: `docs/ops/README.md`
- Plans / acceptance criteria: `docs/plan/README.md`

Note: most docs are currently written in Chinese.

## Install (GitHub Releases)

Release artifacts are **Linux musl static binaries**:

- `x86_64`
- `aarch64`

Install both `xp` and `xp-ops` (downloads `checksums.txt` and verifies SHA256; installs to `/usr/local/bin` by default):

```bash
curl -fsSLO https://raw.githubusercontent.com/IvanLi-CN/xp/main/scripts/install-from-github.sh
sh install-from-github.sh
```

Verify:

```bash
xp --version
xp-ops --version
```

Notes:

- Installs the latest stable release (`releases/latest`) by default.
- If you cannot write to `/usr/local/bin`, the script will try to use `sudo`. You can also pass `--install-dir` (or set `XP_INSTALL_DIR`) to install into a user-writable directory.

## Runtime model (important)

- `xp` binds loopback by default: `127.0.0.1:62416` (HTTP).
- Inter-node connectivity is based on `Node.api_base_url` (recommended to be an **HTTPS origin**). Provide HTTPS via your reverse proxy / tunnel / mesh and forward to the local loopback HTTP listener.
- `xray` runs locally and should expose its gRPC API on loopback (default: `127.0.0.1:10085`). See `docs/desgin/xray.md`.

## Quickstart (local development)

Prerequisites:

- Rust toolchain: `rust-toolchain.toml`
- Bun: `.bun-version`

Install repo tooling (commitlint + dprint) and enable Git hooks:

```bash
bun install
lefthook install
```

Build the embedded admin UI assets (`cargo build` embeds `web/dist`; missing assets will fail the build):

```bash
cd web
bun install
bun run build
```

Initialize a fresh data dir and start `xp`:

```bash
export XP_DATA_DIR=./data-dev
export XP_ADMIN_TOKEN=testtoken

cargo run -- init --api-base-url http://127.0.0.1:62416
cargo run
```

Open:

- Admin UI: `http://127.0.0.1:62416/`
- Health: `http://127.0.0.1:62416/api/health`

Run the web dev server (HMR) with backend proxying:

```bash
cd web
VITE_BACKEND_PROXY=http://127.0.0.1:62416 bun run dev
```

## Configuration

`xp` supports both CLI flags and environment variables (see `src/config.rs`). Common settings:

| Setting                             | Env                           | Default                   | Description                                   |
| ----------------------------------- | ----------------------------- | ------------------------- | --------------------------------------------- |
| `--bind <ADDR>`                     | -                             | `127.0.0.1:62416`         | `xp` HTTP bind address                        |
| `--data-dir <PATH>`                 | `XP_DATA_DIR`                 | `./data`                  | Data directory (identity, Raft, snapshots, …) |
| `--xray-api-addr <ADDR>`            | `XP_XRAY_API_ADDR`            | `127.0.0.1:10085`         | Local `xray` gRPC API address                 |
| `--xray-health-interval-secs <SECS>` | `XP_XRAY_HEALTH_INTERVAL_SECS` | `2`                      | Xray gRPC probe interval (`1..=30`)           |
| `--xray-health-fails-before-down <N>` | `XP_XRAY_HEALTH_FAILS_BEFORE_DOWN` | `3`                 | Consecutive probe failures to mark down (`1..=10`) |
| `--xray-restart-mode <MODE>`        | `XP_XRAY_RESTART_MODE`        | `none`                    | Restart strategy (`none|systemd|openrc`)      |
| `--xray-restart-cooldown-secs <SECS>` | `XP_XRAY_RESTART_COOLDOWN_SECS` | `30`                   | Min seconds between restart requests          |
| `--xray-restart-timeout-secs <SECS>` | `XP_XRAY_RESTART_TIMEOUT_SECS` | `5`                    | Restart command timeout                       |
| `--xray-systemd-unit <UNIT>`        | `XP_XRAY_SYSTEMD_UNIT`        | `xray.service`            | systemd unit name                             |
| `--xray-openrc-service <NAME>`      | `XP_XRAY_OPENRC_SERVICE`      | `xray`                    | OpenRC service name                           |
| `--admin-token <TOKEN>`             | `XP_ADMIN_TOKEN`              | `""`                      | Admin bearer token                            |
| `--node-name <NAME>`                | -                             | `node-1`                  | Node display name                             |
| `--access-host <HOST>`              | -                             | `""`                      | Host used for subscription output             |
| `--api-base-url <ORIGIN>`           | -                             | `https://127.0.0.1:62416` | Public/reachable API origin for this node     |
| `--quota-poll-interval-secs <SECS>` | `XP_QUOTA_POLL_INTERVAL_SECS` | `10`                      | Quota polling interval (`5..=30`)             |
| `--quota-auto-unban <BOOL>`         | `XP_QUOTA_AUTO_UNBAN`         | `true`                    | Auto-unban on cycle rollover                  |

Notes:

- Admin endpoints require `Authorization: Bearer <XP_ADMIN_TOKEN>` (see `docs/desgin/api.md`).
- If `XP_ADMIN_TOKEN` is an empty string, requests with an empty bearer token will pass; for production, always set a strong random non-empty token.

Example:

```bash
	XP_ADMIN_TOKEN="$(openssl rand -hex 32)" \
	XP_DATA_DIR=/var/lib/xp/data \
	XP_XRAY_API_ADDR=127.0.0.1:10085 \
	XP_XRAY_HEALTH_INTERVAL_SECS=2 \
	XP_XRAY_HEALTH_FAILS_BEFORE_DOWN=3 \
	XP_XRAY_RESTART_MODE=systemd \
	xp
```

## API quick reference

- Health: `GET /api/health`
- Cluster info: `GET /api/cluster/info`
- Join token (admin, leader): `POST /api/admin/cluster/join-tokens`
- Join cluster: `POST /api/cluster/join`
- Admin API: `/api/admin/*` (Nodes/Endpoints/Users/Grants/Quota/Alerts, …)
- Subscription: `GET /api/sub/{subscription_token}` (Base64 by default; `?format=raw|clash`)

Full contracts:

- `docs/desgin/api.md`
- `docs/desgin/subscription.md`

## Operations & upgrades

See `docs/ops/README.md` for:

- systemd/OpenRC examples
- `XP_DATA_DIR` layout + backup guidance
- `xp-ops` upgrade/rollback strategy (GitHub Releases)
- optional public access via Cloudflare Tunnel

## Development & testing

Backend:

```bash
cargo test
cargo fmt
cargo clippy -- -D warnings
```

Frontend:

```bash
cd web
bun run lint
bun run typecheck
bun run test
```

E2E (Playwright):

```bash
cd web
bun run test:e2e
```

Local `xray` integration tests (Docker required):

```bash
./scripts/e2e/run-local-xray-e2e.sh
```

Demo seed data (requires admin token):

```bash
XP_ADMIN_TOKEN=testtoken node scripts/dev/seed-m6-demo-data.js
```

## Repository layout

- `src/`: Rust cluster manager (`xp`) and core logic
- `src/bin/xp-ops.rs`: ops tool (`xp-ops`)
- `web/`: Vite + React admin UI (TanStack Router/Query, Tailwind + DaisyUI)
- `docs/desgin/`: design docs
- `docs/ops/`: ops examples and templates
- `docs/plan/`: plans, milestones, acceptance criteria
