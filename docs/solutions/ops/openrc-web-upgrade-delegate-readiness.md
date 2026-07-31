---
title: OpenRC Web upgrade readiness must cross the doas boundary
module: ops
problem_type: upgrade-delegate-readiness
component: web-upgrade
tags:
  - openrc
  - doas
  - openDoas
  - privilege-boundary
status: active
related_specs:
  - docs/specs/nq4ha-web-local-upgrade/SPEC.md
---

# OpenRC Web upgrade readiness must cross the doas boundary

## Context

Web-triggered upgrades run from the unprivileged `xp` service. OpenRC hosts use a root one-shot
`xp-upgrade` service, with doas allowing exactly `/sbin/rc-service xp-upgrade start`. Operators
correctly keep `/etc/doas.conf` at `0600 root:root`.

## Symptoms

- The Web version popover reports `missing installed upgrade delegate`.
- A root audit shows an executable `/etc/init.d/xp-upgrade` and the fixed start rule.
- `xp` cannot read `/etc/doas.conf`, which is expected rather than an installation failure.

## Root Cause

Readiness code tried to inspect `/etc/doas.conf` as the `xp` user. That conflates policy
confidentiality with policy absence. OpenDoas `-C` is not an alternative: it drops privileges
before parsing the configuration, so it still cannot validate a root-only policy file.

## Resolution

- Install a root-owned, mode `0755` fixed helper at
  `/usr/local/libexec/xp-openrc-upgrade-trigger`.
- Permit the `xp` user to run only
  `doas -n /usr/local/libexec/xp-openrc-upgrade-trigger --check`.
- Make the helper accept only `--check`; as root, it verifies that
  `/etc/init.d/xp-upgrade` is executable and that `/etc/doas.conf` has the exact fixed start
  rule.
- Treat a successful helper exit as OpenRC delegate readiness. The actual trigger remains the
  separate, non-interactive fixed command
  `doas -n /sbin/rc-service xp-upgrade start`.
- The already-running pre-helper `xp-ops` process cannot execute release code that it has not yet
  installed. After the first ordinary upgrade across this boundary, require the root operator to
  run `xp-ops init` once. Do not start the one-shot runner from a readiness check as a migration
  shortcut: a missing helper must stay unsupported until that explicit root action.

## Guardrails / Reuse Notes

- Do not relax `/etc/doas.conf` permissions and do not grant wildcard commands.
- The readiness helper must never call `rc-service` or the upgrade runner.
- Check both the helper and runner assets before declaring support; a stale policy alone is not
  enough.
- Regression coverage should use real Alpine OpenDoas with an `xp` user and a `0600 root:root`
  policy. Include the valid policy, a removed fixed start rule, a missing helper, and invalid helper
  arguments.

## References

- `src/upgrade_job.rs`
- `src/ops/init.rs`
- `docs/ops/openrc/xp-upgrade-trigger`
- `docs/ops/openrc/doas-xp-upgrade.conf`
- `scripts/testbox/run-web-local-upgrade-e2e.sh`
