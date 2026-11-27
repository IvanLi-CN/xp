# Implementation

## Current State

- `xp-ops xp recover-single-node` is implemented with `--yes`, `--dry-run`, and backup controls.
- The command rewrites local Raft membership only for a confirmed permanent quorum-loss incident.
- Recovery never deletes a DesiredState Node or Endpoint: retired nodes may still be serving
  traffic, and their endpoints must survive a rejoin unchanged.
- Recovery leaves `missing_desired_members` non-empty by design. Each retired node is then either
  rejoined under its existing identity or removed through an explicit permanent-absence eviction.
- `docs/ops/README.md` contains the operator recovery procedure.

## Delivery Milestones

- Provide the recovery command with default backup, dry-run, and confirmation safeguards.
- Keep a three-node Compose regression path for quorum loss, recovery, and rejoin.
- Maintain the disaster-recovery procedure in the operations documentation.
