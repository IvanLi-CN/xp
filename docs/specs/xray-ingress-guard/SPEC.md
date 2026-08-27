# Host-managed Xray Ingress Guard

## Related ADRs

- [ADR 0002](../../adr/0002-xray-ingress-guard-cgroup-boundary.md)

## Problem

`hk2` is a constrained host-managed node. Its observed CPU spikes are caused by
Xray processing public, unauthenticated TCP/TLS/REALITY connection attempts,
not by the `xp` control plane or cloudflared. Xray has to perform work before it
can distinguish a valid client from a generic Internet probe. Changing a public
port only moves that work to another port.

## Goals

- Drop excess public TCP connection attempts before they reach Xray.
- Protect every current and future non-loopback TCP listener owned by the Xray
  service, without tracking dynamic endpoint ports.
- Keep `xp` unprivileged: no firewall capability, no `sudo`/`doas` rule, and no
  new Web mutation path.
- Integrate activation, status, tuning, and removal into root-run `xp-ops`.
- Keep existing XHTTP connections and loopback control-plane listeners outside
  the admission budget.
- Make an enabled guard durable across Xray restarts and host reboots without a
  resident guard process.

## Non-goals

- Docker/Compose deployment support.
- Protocol-aware classification of valid versus invalid VLESS/REALITY clients.
- Terminating already-established connections based on an inferred abuse score.
- Replacing provider L3/L4 DDoS protection or protecting non-Xray processes.
- Adding a Web button, API endpoint, or privilege delegation for `xp`.

## Supported Boundary

The feature supports only XP-generated host-managed `xray.service` on systemd
and the XP-generated `xray` script on OpenRC. It requires Linux cgroup v2,
nftables, and kernel support for nft's `socket cgroupv2` expression. A host
which fails a pre-package requirement returns a concrete unsupported reason and
remains unchanged. If explicit `--yes` first installs a missing nftables package
and its subsequent capability check fails, that package may remain installed,
but no firewall rule, service asset, or XP configuration is changed.

Custom service names, manually managed Xray units, container nodes, and kernels
without the required nft socket capability are not silently adapted. The
operator must keep the existing deployment or use a separately owned firewall
configuration.

## Admission Contract

### Eligible traffic

A packet is eligible only when all of the following hold:

- It is an initial TCP SYN arriving through a non-loopback interface.
- nft resolves its destination listener to the current Xray service cgroup.
- That listener is a Protected Xray Socket.

The guard does not match an endpoint port list. A listener created later by the
same Xray process is covered immediately. Xray gRPC, XP's API, the VLESS canary,
and other loopback-only sockets remain outside the scope because they do not
receive non-loopback traffic.

### `small-vps` profile

| Budget                      | Value                  | Scope                                  |
| --------------------------- | ---------------------- | -------------------------------------- |
| Global new TCP SYN rate     | 8 per second, burst 20 | All Protected Xray Sockets (IPv4/IPv6) |
| Per-source new TCP SYN rate | 3 per second, burst 8  | Each IPv4 source and each IPv6 source  |
| Excess action               | Silent drop            | Only the over-limit initial SYN        |

The global budget stops distributed probing. Bounded per-source meters stop a
single source from consuming the global budget. Each address-family meter has a
maximum of 1,024 entries and a 60-second idle expiry; if one is full, the global
budget remains the fallback protection. No connection-count rule is included in
v1: it would introduce connection-tracking state and false positives on a small
host without reducing the initial TLS/REALITY work as directly as SYN admission
does.

The profile applies only to new TCP connections. XHTTP multiplexing, traffic on
an established connection, and ordinary TCP retransmission are not rate-limited
by it. Root may later use a validated `set-limits` operation to move to a custom
profile atomically; it never requires a port migration or an `xp` restart.

### Ruleset ownership

`xp-ops` owns exactly `table inet xp_ingress_guard`. The table has a managed
identity marker, an inline `socket cgroupv2` selector for the current Xray
service cgroup, named global/per-source limit state, and counters for admitted
and dropped SYNs.

Its input base chain drops only eligible over-limit packets and otherwise
returns. It does not set a default policy, accept packets, flush another table,
write `/etc/nftables.conf`, or enable the distribution's `nftables` service.
An existing table with this name but without XP's exact ownership marker is an
`ownership_conflict`, never an overwrite target.

Before changing the owned table, the root runner renders an allowlisted nft
program from typed configuration, validates it with nft's check mode, applies
the one-table transaction, and reads the table back as JSON. The table is
Verified only if its cgroup selector, profile values, and required counters
match the expected program.

### Cgroup refresh and startup behavior

Cgroup identity can change when a service restarts, so a static rule is not
sufficient. The Guard Refresh Hook runs once on every Xray service start after
the Xray cgroup exists and before Xray executes. It resolves its own cgroup,
refreshes the owned table, verifies it, writes a volatile current-cgroup permit,
and exits. The permit is root-owned; a non-root process cannot create or modify
it.

For systemd, the generated unit keeps `User=xray` and `Group=xray`. Its
root-only pre-start action is enabled through the legacy-compatible
`PermissionsStartOnly=true` mechanism. Its unprivileged exec gate checks the
current-cgroup permit and immediately `exec`s Xray. A missing permit exits with
status `77`; `RestartPreventExitStatus=77` leaves the unit failed and prevents
an automatic retry loop. The gate is not resident after Xray starts.

For OpenRC, OpenRC establishes the service cgroup before `start_pre`. The
generated `start_pre` invokes the root runner and a failure prevents the
supervisor from starting Xray. A supervise-daemon child respawn remains in the
same service cgroup, so its verified rule remains valid; a full service restart
runs `start_pre` again.

In enforced mode, a missing or invalid Verified Guard Ruleset means Xray does
not start. There is no unprotected fallback. A profile update that cannot be
verified preserves the already-active ruleset and current Xray process rather
than partially applying the new configuration.

Observe mode is deliberately different: it keeps the direct Xray startup path
and treats a runner error as degraded diagnostics, not a service-start block.
It is never selected implicitly by `enable` and cannot replace the enforced
contract after an immediate `small-vps` activation.

## `xp-ops` Interface

All mutating commands require root and explicit `--yes`; every mutation also
supports `--dry-run`. None of these commands is exposed through `xp`, the Web
API, polkit, sudo, or doas.

```text
xp-ops ingress-guard enable --profile small-vps --yes
xp-ops ingress-guard observe --profile small-vps --yes
xp-ops ingress-guard set-limits \
  --global-rate 8 --global-burst 20 --source-rate 3 --source-burst 8 --yes
xp-ops ingress-guard status [--json]
xp-ops ingress-guard disable --yes
```

`enable` is the immediate existing-host path. It installs the distribution
`nftables` package only when needed, validates the host, installs the root-owned
configuration and generated service assets, then performs one Xray restart into
enforced mode. Before the first configuration write, an unsupported host stays
unchanged apart from an explicitly requested dependency package installation.
Once enforced assets are committed, a startup verification failure leaves Xray
stopped instead of restoring an unprotected startup path.

`observe` uses the same selector and counters but records would-drop events
without dropping packets. It is an explicit diagnostic mode, not a prerequisite
for the immediate `small-vps` rollout. `disable` is a root-only, explicit
emergency action; it removes only XP-owned assets and the owned nft table.

`status` is a root-local read path. It reports mode, profile, capability result,
current verification result, last root-runner error code, and aggregate nft
counters. It never prints remote addresses, endpoint ports, certificates, or
credentials.

## Persistent and Volatile State

| Path or object                   | Owner       | Purpose               |
| -------------------------------- | ----------- | --------------------- |
| `/etc/xp-ops/ingress-guard.toml` | `root:root` | Typed config/identity |
| `/run/xp-ingress-guard/`         | `root`      | Cgroup permit/status  |
| `table inet xp_ingress_guard`    | `kernel`    | Rules and counters    |

The persistent configuration has no user-controlled fields. The volatile permit
does not survive boot; the Xray start path recreates it before opening a public
listener. File removal and cleanup must reject symlinks and affect only the
listed owned paths.

## Security and Failure Model

- `xp` may retain its existing narrow authority to request an Xray restart, but
  cannot influence the root runner's arguments, profile, cgroup target, nft
  program, or permit.
- A restart requested by `xp` still traverses the same service-manager-owned
  hook, so it cannot bypass enforcement.
- A `set-limits` failure leaves the verified old table intact. An `enable`
  preflight failure before assets are committed leaves host guard assets
  unchanged; an explicitly installed dependency package may remain present.
- An enforced service-start failure blocks Xray and records a local reason; it
  does not start Xray without the guard and does not repeatedly spin a root
  process.
- Root may explicitly disable the guard to recover service availability. That
  action is intentionally visible, requires `--yes`, and is not automated.

## Upgrade and Deployment Contract

- Host-managed `xp-ops init` renders the guard-aware Xray service asset whenever
  root-owned guard configuration is present and preserves a verified enabled
  profile during reinitialization.
- Host-managed `xp-ops upgrade` must preserve enabled guard configuration,
  install compatible runner/exec-gate assets before restarting Xray, and verify
  the guard after the restart. It must restore the prior compatible guarded
  assets if the upgrade transaction rolls back.
- An upgrade must never resolve an enabled guard failure by reverting to an
  unguarded Xray service. If compatible guarded recovery cannot be established,
  Xray remains stopped with the terminal reason.
- New deployment remains explicit: `xp-ops ingress-guard enable` is the
  one-command post-deploy activation. A future `deploy` flag may compose that
  same operation, but it must remain opt-in and reuse the exact root-only
  transaction.

## Acceptance Criteria

- Given a supported systemd/OpenRC host, when `enable --profile small-vps --yes`
  succeeds, then Xray is running as `xray`, the owned table is verified, and no
  new privilege rule exists for `xp`.
- Given a public TCP listener in the Xray cgroup, when more than the global or
  per-source budget arrives, then excess SYNs are silently dropped before the
  listener receives them and the corresponding counter increases.
- Given Xray creates a new non-loopback TCP listener after activation, when a
  SYN reaches it, then the cgroup selector applies without a port-specific rule
  update.
- Given a loopback Xray listener, when loopback traffic reaches it, then no
  ingress-guard admission counter changes and the traffic proceeds normally.
- Given an enforced Xray restart whose nft preparation or read-back verification
  fails, then Xray does not start; systemd does not enter a restart loop and
  OpenRC does not launch supervise-daemon.
- Given an active verified guard, when `set-limits` fails its check or apply
  phase, then the prior table and running Xray remain active.
- Given an unsupported kernel, custom Xray service, container node, or foreign
  same-name nft table, when `enable` runs, then it returns a specific error and
  leaves host firewall and services unchanged. A dependency package explicitly
  installed before a later nft capability error may remain installed.
- Given `disable --yes`, when it succeeds, then only XP-owned guard state is
  removed; unrelated nft tables, host policies, and Xray endpoint configuration
  remain untouched.

## Limits

This is an admission-control solution, not an attack classifier. A distributed
source that stays below the selected global budget can still cause work, and
the host still pays kernel packet-processing cost before nft drops a packet.
Existing Xray `handshake=4` and idle policies remain the timeout layer for work
that was admitted. Provider-side L3/L4 mitigation remains appropriate for a
packet-volume attack that overwhelms the host before its input hook can help.
