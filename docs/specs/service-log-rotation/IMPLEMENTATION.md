# Implementation

## Current State

- No implementation is recorded for the log-rotation contract in `SPEC.md`.
- The documented behavior remains pending delivery across supported systemd and OpenRC hosts.

## Delivery Scope

- Implement the frozen `xp-ops init` CLI, logrotate artifact, runner, tests, and operations docs.

## Delivery Milestones

- `xp-ops init` generates `/etc/logrotate.d/xp-ops` with derived Xray paths,
  `copytruncate`, and configurable `100MiB` / seven-day limits.
- Install and enable a runner for systemd and Alpine OpenRC using an independent status file.
- Cover `--root`, `--dry-run`, missing-dependency failures, and operations documentation.
