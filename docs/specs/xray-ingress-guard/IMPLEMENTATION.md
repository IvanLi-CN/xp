# Host-managed Xray Ingress Guard Implementation

## Source Boundaries

- `src/ops/cli.rs`: add the root-only `ingress-guard` command group and typed
  arguments. Mutating subcommands require `--yes` and expose `--dry-run`.
- `src/ops/ingress_guard.rs` and focused child modules: own typed profile
  parsing, capability checks, nft rendering/read-back, runner status, and the
  hidden service-start commands. Do not place nft string construction in CLI or
  init rendering code.
- `src/ops/paths.rs`: add the root-owned configuration and volatile state paths.
- `src/ops/preflight.rs`: validate root execution, Linux/cgroup-v2/nft support,
  supported init system, and XP-generated Xray service ownership.
- `src/ops/install.rs`: install `nftables` through the existing distro package
  paths only after explicit approval: pacman, apt, yum/dnf-family, or apk.
- `src/ops/init.rs`: render the guarded systemd/OpenRC Xray start assets from
  root-owned guard state while preserving the direct unguarded asset when guard
  state is absent.
- `src/ops/upgrade/**`: carry guarded assets and configuration through the
  host-managed upgrade transaction and its rollback path.

## Command Design

`enable` follows this order:

1. Require root and `--yes`; reject container mode and unmanaged/custom Xray
   services before any write.
2. Resolve distro/init system and install `nftables` if absent.
3. Check cgroup v2, the active managed Xray cgroup, nft socket-cgroup support,
   and absence of an ownership conflict. The socket-expression check uses an
   nft check transaction against the real current service cgroup.
4. Stage root-owned configuration and guard-aware Xray asset changes.
5. Reload the service manager, perform the one Xray restart, and wait for the
   manager's ready state.
6. Read back the JSON representation of `inet xp_ingress_guard`, validate the
   expected identity/profile/cgroup/counters, and report success only then.

The pre-commit stages are all-or-nothing from the operator's view: unsupported
or validation failure does not write firewall or service assets. After guard
assets become enforced, a failed Xray start stays blocked by design. `disable`
is the only explicit route back to an unguarded service.

`set-limits` takes the operation lock, validates all four positive bounded
integers, stages a candidate root-owned configuration, checks a candidate
program, applies the replacement owned table as one nft transaction, verifies
JSON read-back, and commits the staged configuration. It does not restart Xray.
If durable configuration finalization fails after nft application, it restores
the previously verified table before returning an error. A failed operation
therefore retains the old durable configuration and old verified table.

## Nft Rendering

The renderer must construct a typed nft program rather than concatenate
operator strings. It creates or refreshes only `inet xp_ingress_guard` and
includes:

- an inline cgroups-v2 socket selector for the current Xray service-cgroup path;
- one global named token bucket shared by IPv4 and IPv6;
- finite IPv4 and IPv6 source-meter state, each capped at 1,024 entries with a
  60-second idle expiry;
- named aggregate counters for eligible SYNs, global drops, and source drops;
- an input base chain whose only terminal verdict is `drop` for an over-limit
  eligible SYN.

Named counters are declared as table-level objects and referenced with nft's
`counter name <id>` statement. Source meters are match expressions; their
over-limit verdict and counter occur after the meter block so the counter only
records excess source traffic.

The base chain has policy `accept` and returns for every non-drop path. It must
never emit an `accept` verdict that could bypass another owner’s host firewall
chain. It excludes loopback and matches a TCP SYN with no ACK/RST flags before
testing the listener's cgroup selector.

Use nft check mode before application and parse
`nft --json list table inet xp_ingress_guard` after application.

Textual `nft list ruleset` output is not a verification contract. A refresh on
an existing owned table uses one atomic nft transaction; first creation occurs
before any guarded Xray listener is started.

## Init-system Assets

### systemd

The guarded Xray unit keeps `User=xray` and `Group=xray`. It has:

- a root `ExecStartPre` through `PermissionsStartOnly=true`, with execution
  failure ignored only so the main exec gate can report the terminal status;
- an unprivileged internal exec gate as `ExecStart`, which requires the
  current-cgroup permit and immediately replaces itself with the fixed Xray
  command;
- `RestartPreventExitStatus=77`, so an absent permit produces a visible failed
  unit without a restart loop;
- existing crash recovery semantics after a successfully started Xray process.

The root runner must always clear or supersede stale permits by cgroup identity
before a new verification attempt. It writes a permit only after successful nft
read-back verification. The exec gate may read the permit but cannot create,
change, or select it.

Observe mode does not install the systemd exec gate. Its pre-start action records
runner failure locally and returns success so Xray keeps its existing direct
startup path. The enforced renderer is the only renderer that may block Xray.

### OpenRC

The guarded `xray` script adds `start_pre`, which invokes the same root runner.
OpenRC creates and enters the service cgroup before `start_pre`, so the runner
uses its own cgroup rather than accepting a path from configuration or `xp`.
A runner failure returns nonzero from `start_pre`; OpenRC therefore does not
launch `supervise-daemon`. The existing supervisor continues to respawn Xray
only after a successfully verified service start, inside the same cgroup.
For observe mode, `start_pre` records the failed observation and returns success
instead of blocking the service.

## Tests

- Unit tests cover profile bounds, root-only command validation, cgroup-path
  extraction, managed-table ownership checks, typed nft program rendering, JSON
  read-back parsing, status redaction, and no-follow cleanup.
- Init asset tests cover systemd root-prestart/exec-gate/status-77 semantics and
  OpenRC `start_pre` semantics while retaining the `xray` user identity.
- Transaction tests cover no-write unsupported failures, ownership conflicts,
  candidate failure retaining the old profile/table, enable failure blocking the
  guarded start, and explicit disable touching no foreign nft object.
- Linux namespace integration tests run nft against a listener placed in a test
  cgroup. They prove an over-limit SYN is not delivered, a below-limit SYN is
  delivered, loopback is excluded, and a listener added after rule installation
  is protected by cgroup membership rather than port enumeration.
- systemd/OpenRC lifecycle integration runs only in an environment that provides
  the relevant init system and cgroup-v2/nft capabilities. Shared testbox use is
  test infrastructure only; it does not make Docker a supported deployment.

## Delivery Sequence

1. Add pure profile/config/nft-rendering modules and unit tests.
2. Add preflight, package install, status, and explicit root command behavior.
3. Add guarded systemd/OpenRC asset rendering and lifecycle tests.
4. Add atomic enable/set-limits/disable transactions and upgrade preservation.
5. Run Linux namespace and supported-init integration coverage, then perform a
   root-approved `small-vps` activation on hk2 with post-activation status and
   CPU evidence.
