---
title: Systemd Web upgrade runners need direct ExecStart and stale status recovery
module: ops
problem_type: production-upgrade-stuck
component: web-upgrade
tags:
  - systemd
  - xp-ops
  - upgrade-status
status: active
related_specs:
  - docs/specs/nq4ha-web-local-upgrade/SPEC.md
---

# Systemd Web upgrade runners need direct ExecStart and stale status recovery

## Context

Web-triggered local upgrades are started by the unprivileged `xp` service through a narrow root
delegate. On systemd hosts the delegate starts `xp-upgrade.service`, which runs
`xp-ops _upgrade-runner` once and writes the terminal upgrade result into
`${XP_DATA_DIR}/upgrade/status.json`.

## Symptoms

- The Web version popover reports `Upgrade ... is running` indefinitely.
- `/usr/local/bin/xp --version` still reports the old version.
- No `xp-ops` / `xp-upgrade` process is running.
- `systemctl status xp-upgrade.service` shows the one-shot failed quickly.
- `${XP_DATA_DIR}/upgrade/status.json` is still `state: "running"` with
  `message: "upgrade trigger accepted"`.

## Root cause

Do not combine systemd command-line parsing with a shell fallback expression for critical runner
arguments. A unit like this is fragile:

```ini
ExecStart=/bin/sh -c 'exec /usr/local/bin/xp-ops _upgrade-runner \
  --data-dir "${XP_DATA_DIR:-/var/lib/xp/data}"'
```

systemd processes dollar expressions in command lines before the shell sees them. The shell may
receive an already-expanded empty `--data-dir` value, causing clap to fail before `_upgrade-runner`
can read the request or write terminal status.

The second failure is status ownership: the Web start API has already written `running`, but a
pre-runner one-shot failure has no process left that can write `failed`.

## Resolution

- Let the unit pass `XP_DATA_DIR` through environment only:

```ini
Environment=XP_DATA_DIR=/var/lib/xp/data
EnvironmentFile=-/etc/xp/xp.env
ExecStart=/usr/local/bin/xp-ops _upgrade-runner
```

- Keep `_upgrade-runner` responsible for reading `XP_DATA_DIR` from the environment and for its own
  default.
- When reading upgrade status, reconcile active durable states against local runner facts. If
  `xp-upgrade.service` is loaded and `ActiveState=failed`, write and return a `failed` status with a
  short message that includes the systemd result and exit status.
- Preserve the normal active-job guard when no delegate failure is known; a fresh `running` /
  `restarting` job must still block duplicate starts with `409 upgrade_already_running`.

## Guardrails / Reuse notes

- Unit tests for systemd upgrade units should assert that `ExecStart` does not contain `/bin/sh -c`,
  `--data-dir`, or `${XP_DATA_DIR`.
- Status recovery tests should cover both sides: failed delegate converts active status to `failed`;
  no delegate failure leaves active status active.
- Production recovery after this bug is not only a code deploy. Reinstall the delegate with
  `xp-ops init` or the host-managed deployment path, clear or rewrite the stale active status, then
  retry the upgrade.

## References

- `src/ops/init.rs`
- `src/upgrade_job.rs`
- `docs/ops/systemd/xp-upgrade.service`
- `docs/specs/nq4ha-web-local-upgrade/SPEC.md`
