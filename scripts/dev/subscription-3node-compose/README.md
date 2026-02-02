# Local 3-node subscription regression environment (apxdg)

This directory provides a docker-compose based 3-node `xp` cluster on a single dev machine, plus scripts to:

- reset (wipe volumes)
- bring up a 3-node cluster
- seed deterministic subscription data
- verify subscription output on all 3 nodes

## Prerequisites

- Docker Engine
- Docker Compose (`docker compose` or `docker-compose`)
- BuildKit supported (the script sets `DOCKER_BUILDKIT=1`)
- `python3` on host (used for JSON parsing / port allocation)

## Quick start

```sh
./scripts/dev/subscription-3node-compose/run.sh reset-and-verify
```

## Useful commands

```sh
./scripts/dev/subscription-3node-compose/run.sh reset
./scripts/dev/subscription-3node-compose/run.sh up
./scripts/dev/subscription-3node-compose/run.sh seed
./scripts/dev/subscription-3node-compose/run.sh verify
./scripts/dev/subscription-3node-compose/run.sh urls
./scripts/dev/subscription-3node-compose/run.sh logs
```

## Notes

- The cluster runs `xp` HTTP internally and uses `socat` for HTTPS termination (`https://xp{1,2,3}:6443` inside the compose network).
- Certificates are generated from the cluster CA produced by `xp init` and stored under each node volume.
- Seed data creates:
  - 1 user (`alice`)
  - 4 endpoints across 2 nodes
  - 4 enabled grants with the same `note` (`"same"`) to reproduce name-collision behavior.
