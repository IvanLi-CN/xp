---
title: Docker testbox validates host-managed fresh joins
module: cluster onboarding
problem_type: deployment integration validation
component: xp-ops deploy and init managers
tags:
  - fresh-join
  - staged-join
  - docker
  - codex-testbox
  - systemd
  - openrc
  - xp-ops
status: active
related_specs:
  - 38wmj-cluster-node-onboarding
---

# Docker Testbox Validates Host-Managed Fresh Joins

## Context

The staged fresh-join protocol returns durable bootstrap material before the leader waits for
learner catch-up. The joiner must then persist that material, start its authenticated XP runtime,
and allow the leader to promote it. This contract has to work through the supported host-managed
`xp-ops deploy` paths as well as the single-image runtime.

Unit and single-image tests cannot prove that a systemd or OpenRC host writes its managed files,
starts Xray before XP, survives service restarts, and joins through the authenticated Raft path.
The shared testbox is the permitted Docker-only environment for that integration coverage.

## Resolution

Use `scripts/testbox/run-host-managed-fresh-join-e2e.sh`. The local launcher maps the checkout to
a unique `/srv/codex/workspaces/<user>/<repo>__<path-hash>/runs/<run-id>` directory and invokes a
synced remote driver. Keeping the driver as a file, instead of piping it over SSH, is necessary:
Docker Compose may read standard input and otherwise consume the rest of the control script.
The run id includes a random nonce in addition to UTC time and the candidate SHA, so concurrent
launches from the same checkout cannot share Compose projects, volumes, receipts, or cleanup paths.

The remote driver builds static musl `xp` and `xp-ops` binaries in a restricted Docker builder.
It then rebuilds the leader image from that run's `xp` artifact, rather than trusting the
preloaded candidate image. This keeps the leader API and host-managed joiner on the same exact
head. It creates an isolated Compose project containing:

- a normal official single-image leader and a TLS sidecar;
- a Debian systemd PID 1 node and an Alpine OpenRC PID 1 node;
- one local TLS sidecar for each host node; and
- a local Xray release fixture built from the candidate image's Xray binary.

The fixture retains the real `xp-ops deploy` Xray release download/install path without making
the join test depend on GitHub or a public release service. Each fresh node runs the official
`xp-ops deploy --enable-services` command, not `xp-ops container run`.

The run asserts that both managers report Xray and XP ready, the authenticated
`/api/cluster/info` endpoint reports `follower`, metadata and the admin-token hash are durable,
both XP services can restart, and the leader lists `leader`, `systemd`, and `openrc` by the
public `node_name` field. A successful run writes a non-sensitive receipt in the mapped
`receipts` directory and removes only that run's Compose project, volumes, temporary images, and
run directory.

## Guardrails

- Run this only through `$shared-testbox-runner` on `codex-testbox`; do not use a VPS, a
  production cluster, or local Docker.
- Keep every remote write under `/srv/codex/**`, use the generated Compose project name, and never
  use Docker-wide prune commands.
- Preserve authenticated Raft traffic. This harness must not disable authentication, copy Raft
  state, expose an unauthenticated Raft endpoint, or replace host-managed deploy with the
  single-image wrapper.
- Alpine/OpenRC can log failed attempts to create cgroup subdirectories when the shared LXC cgroup
  mount is read-only. Treat that as an environment diagnostic only when `rc-service` status,
  follower state, and post-restart checks all pass.
- The durable cargo, rustup, and target caches stay under the mapped testbox workspace. They are
  build caches, not cluster state; per-run runtime resources must still be cleaned.

## Verification

Run:

```bash
scripts/testbox/run-host-managed-fresh-join-e2e.sh
```

The pass receipt records `deploy=official-xp-ops`, both follower roles, and preserved identity
after restart. The owner-facing onboarding contract remains in the owning spec and ops guide;
this solution documents the reusable Docker-only validation method.

## References

- `scripts/testbox/run-host-managed-fresh-join-e2e.sh`
- `scripts/testbox/host-managed-fresh-join-remote.sh`
- `scripts/testbox/compose-host-managed-fresh-join.yml`
- `docs/specs/38wmj-cluster-node-onboarding/SPEC.md`
- `docs/ops/README.md`
