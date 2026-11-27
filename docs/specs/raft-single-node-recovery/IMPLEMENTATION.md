# Implementation

## Current State

- `xp-ops xp recover-single-node` is implemented with `--yes`, `--dry-run`, and backup controls.
- The command rewrites local Raft membership only for a confirmed permanent quorum-loss incident.
- Recovery also prunes every DesiredState Node and Endpoint outside the new single-node membership.
  Without that pruning the surviving leader keeps failing the `missing_desired_members` invariant,
  which blocks both fresh join and remove-node, so a recovered cluster could never be repaired.
- The default backup covers `raft/` together with the pruned DesiredState records.
- `docs/ops/README.md` contains the operator recovery procedure.

## Delivery Milestones

- Provide the recovery command with default backup, dry-run, and confirmation safeguards.
- Keep a three-node Compose regression path for quorum loss, recovery, and rejoin.
- Maintain the disaster-recovery procedure in the operations documentation.
