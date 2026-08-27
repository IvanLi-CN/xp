# Evict an unreachable mapped voter through the durable removal lifecycle

An unreachable voter can block ordinary node deletion because that operation correctly requires
the lifecycle capability from every current voter. A raw Raft edit would recover availability but
would leave DesiredState Nodes and endpoints out of sync. The supported exception is therefore a
leader-local, signed `xp-ops` command that first proves one exact non-leader voter and then
excludes only that target from the retained-voter capability check.

The command requires a dry-run membership fingerprint, a clean linearizable membership view, no
joint configuration or active operation, an exact DesiredState node mapping, and an exact endpoint
snapshot. Apply records the existing `RemoveNode` operation rather than introducing another
mutation path. The established resumer then removes the voter, verifies absence, atomically deletes
the node and confirmed endpoints, and queues runtime/history cleanup.

## Considered options

- Relax ordinary admin node deletion: rejected because a transiently unavailable voter must not
  silently become removable from the public admin surface.
- Reuse orphan-voter repair: rejected because its invariant requires no DesiredState mapping and
  deliberately preserves nodes and endpoints.
- Edit Raft membership or snapshots manually: rejected because it bypasses durable intent,
  endpoint cleanup, compare-and-swap evidence, and uncertain-result recovery.
