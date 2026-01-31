# Ops: running `xp` with local `xray`

This directory contains sample service definitions and an environment template to operate `xp` long-term.

## Minimal runtime assumptions

- `xp` runs as a local HTTP admin/API server and binds loopback by default (`127.0.0.1:62416`).
- `xray` runs locally and exposes its gRPC API on loopback by default (`127.0.0.1:10085`).
- `xp` talks to `xray` via gRPC at `XP_XRAY_API_ADDR`.
- `xp` periodically probes `xray` and exposes status via `GET /api/health` (`xray.*` fields). On `down -> up`, `xp` requests a full reconcile.
- `xray` is supervised by the init system (systemd/OpenRC). `xp` does not spawn `xray`, but it can request a restart through the init system (requires a minimal permission policy installed by `xp-ops`).

## Optional: public access via Cloudflare Tunnel

If you want to reach `xp` from the public Internet without opening inbound ports, see:

- `docs/ops/cloudflare-tunnel.md`

Notes:

- `xp-ops deploy` supports passing the Cloudflare API token via `--cloudflare-token` (riskier) or `--cloudflare-token-stdin` (preferred over the flag).
- Token resolution priority for deploy is: `flag/stdin` → `CLOUDFLARE_API_TOKEN` → `/etc/xp-ops/cloudflare_tunnel/api_token`.

## `xp-ops tui` (deploy wizard)

`xp-ops tui` provides an interactive deploy wizard for `xp-ops deploy`.

Note:

- The TUI assumes `xp` is already installed at `/usr/local/bin/xp` (e.g., via `scripts/install-from-github.sh`).

Persistence:

- Deploy settings are stored at `/etc/xp-ops/deploy/settings.json`.
- Cloudflare API token is stored at `/etc/xp-ops/cloudflare_tunnel/api_token`.
  - The TUI never prints the token value; it shows `(saved)` or a mask.
  - Leaving the token input empty keeps the existing token unchanged (does not delete or overwrite it).

Key bindings:

- Focus: `Tab` / `Shift+Tab`, `↑` / `↓`, mouse left click
- Editing: type directly into the focused field (use `Backspace` to delete; paste supported)
- Toggles: `Space` (or `Enter`) on boolean fields
- Commands:
  - `Ctrl+S`: save settings (and token if non-empty)
  - `Ctrl+D`: autosave, then deploy (autosave also runs in `dry_run`)
  - `Ctrl+Q`: quit (asks to save if there are unsaved changes)

Quit confirmation (only when there are unsaved changes):

- `Ctrl+S`: save and exit
- `Ctrl+Q`: exit without saving
- `Esc` / `Enter`: cancel

## Environment variables

These names and defaults are sourced from `src/config.rs`.

Required (or commonly set):

- `XP_DATA_DIR` (default: `./data`)
  - Path to the node data directory. See layout below.
- `XP_ADMIN_TOKEN` (default: empty string)
  - Optional bearer token for admin endpoints. Leaving it empty effectively disables token checks.
  - If you deployed via `xp-ops deploy`, the token is stored in `/etc/xp/xp.env` as `XP_ADMIN_TOKEN`.
    - Show it on the server: `sudo xp-ops admin-token show` (or `--redacted`).
- `XP_XRAY_API_ADDR` (default: `127.0.0.1:10085`)
  - Address of the local `xray` gRPC API.
- `XP_XRAY_HEALTH_INTERVAL_SECS` (default: `2`, allowed range `1..=30`)
  - Probe interval for `xray` gRPC availability.
- `XP_XRAY_HEALTH_FAILS_BEFORE_DOWN` (default: `3`, allowed range `1..=10`)
  - Consecutive probe failures before reporting `xray.status=down`.
- `XP_XRAY_RESTART_MODE` (default: `none`)
  - `none|systemd|openrc`. When enabled, `xp` requests an init-system restart after `xray` is marked down.
- `XP_XRAY_RESTART_COOLDOWN_SECS` (default: `30`, allowed range `1..=3600`)
  - Minimum time between restart requests (prevents restart storms).
- `XP_XRAY_RESTART_TIMEOUT_SECS` (default: `5`, allowed range `1..=60`)
  - Timeout for the restart command invocation.

Optional quota knobs:

- `XP_QUOTA_POLL_INTERVAL_SECS` (default: `10`, allowed range `5..=30`)
- `XP_QUOTA_AUTO_UNBAN` (default: `true`)

An example env file is provided at `docs/ops/env/xp.env.example`.

## Data directory layout (`XP_DATA_DIR`)

The runtime persists its identity, raft state, and snapshots under `XP_DATA_DIR`. This layout matches the code in:

- `src/cluster_metadata.rs`
- `src/raft/node.rs`
- `src/state.rs`

```
${XP_DATA_DIR}/
  cluster/
    metadata.json
    cluster_ca.pem
    cluster_ca_key.pem
    node_cert.pem
    node_key.pem
    node_csr.pem
  raft/
    wal/
    snapshots/
  state.json
  usage.json
```

Notes:

- `cluster/` holds long-lived identity and TLS assets. Treat `cluster_ca_key.pem` as sensitive (private key).
- `raft/` holds the raft write-ahead log and snapshots.
- `state.json` and `usage.json` are JSON snapshots; on schema mismatches, startup fails instead of silently migrating.

## Service examples

### systemd

See:

- `docs/ops/systemd/xp.service`
- `docs/ops/systemd/xray.service`
- (optional) `docs/ops/systemd/cloudflared.service`

Recommended workflow:

1. Copy the unit files to `/etc/systemd/system/`.
2. Copy `docs/ops/env/xp.env.example` to `/etc/xp/xp.env` and edit as needed.
3. Ensure `XP_DATA_DIR` exists and is writable by the service user.
4. Enable and start services:

```
sudo systemctl daemon-reload
sudo systemctl enable --now xray.service
sudo systemctl enable --now xp.service
```

### OpenRC (Alpine-like)

See:

- `docs/ops/openrc/xp`
- `docs/ops/openrc/xray`
- (optional) `docs/ops/openrc/cloudflared`

Suggested workflow:

1. Copy scripts to `/etc/init.d/` and make executable.
2. (Optional) Configure environment variables via OpenRC's `/etc/conf.d/<service>` mechanism.
3. Add to default runlevel and start:

```
sudo rc-update add xray default
sudo rc-update add xp default
sudo rc-service xray start
sudo rc-service xp start
```

## Upgrade and rollback strategy

### Recommended: upgrade via `xp-ops` (GitHub Releases)

`xp-ops` can upgrade both `xp` and `xp-ops` from GitHub Releases (Linux musl assets).

Upgrade both `xp` (installs to `/usr/local/bin/xp` and restarts the service) and `xp-ops`:

```
sudo xp-ops upgrade --version latest
```

Useful flags:

- `--dry-run` prints the resolved release + actions without downloading/writing/restarting.
- `--prerelease` (only with `--version latest`) selects the newest prerelease instead of stable.
- `--repo <owner/repo>` (or `XP_OPS_GITHUB_REPO=<owner/repo>`) overrides the default source repo.

UI notes:

- The Web UI header shows the current `xp` version (clickable) and can check whether a newer stable GitHub Release exists.
- The UI does not perform upgrades; upgrades are still expected to be done via `xp-ops upgrade`.
- If you override the upgrade source repo via `XP_OPS_GITHUB_REPO`, the version check uses the same repo.

Rollback notes:

- The upgrade keeps a backup next to the install path as `<path>.bak.<unix-ts>`.
- On upgrade failures, `xp-ops upgrade` automatically rolls back to the previous `xp` binary.

### Backup before upgrade

Before upgrading the binary, stop the service and back up the entire `XP_DATA_DIR`. The most critical parts are:

- `cluster/` (identity + CA material)
- `raft/` (wal + snapshots)

Example:

```
sudo systemctl stop xp.service
sudo tar -C "$(dirname "$XP_DATA_DIR")" -czf "xp-data-$(date +%Y%m%d%H%M%S).tgz" "$(basename "$XP_DATA_DIR")"
```

### Upgrade steps

1. Stop `xp`.
2. Back up `XP_DATA_DIR`.
3. Deploy the new `xp` binary (and restart).

If `xp` starts cleanly, the upgrade is complete.

### What to do on startup failures / schema mismatches

`xp` validates on-disk schema versions and fails fast on mismatches for:

- `cluster/metadata.json` schema version (`src/cluster_metadata.rs`)
- `state.json` schema version (`src/state.rs`)
- `usage.json` schema version (`src/state.rs`)

If you see startup failures mentioning schema/version mismatch, do not edit these files manually. The safe recovery path is:

1. Stop `xp`.
2. Roll back to the previous `xp` binary (the last known-good version).
3. Restore the `XP_DATA_DIR` backup you took before the upgrade.
4. Start `xp` again.

### `xp init` compatibility check (high-level)

`xp init` initializes `cluster/metadata.json` and then loads/initializes `state.json` using the new node identity.
If `state.json` already exists but does not contain exactly one node matching the new `metadata.json` node ID,
`xp init` fails with a compatibility error.

Practical guidance:

- Do not re-run `xp init` against an existing data dir unless you are intentionally bootstrapping a new cluster.
- For an existing node, upgrade by swapping the binary and keeping the existing `XP_DATA_DIR` (with a backup).
