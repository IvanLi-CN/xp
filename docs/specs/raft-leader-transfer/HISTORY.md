# Raft leader election handoff history

## Lifecycle / Compatibility

- This is a new active topic. Existing Raft membership and snapshot formats are unchanged.
- A target must run a build with the signed `/_internal/raft/elect` route. Older targets fail
  closed instead of attempting a raw Raft edit.

## Replacements / Background

- OpenRaft 0.9 does not expose a target-directed leadership-transfer API. The safe available
  primitive is a local election trigger on the intended target, followed by an observed leader
  metric check.
- Stopping the current leader was rejected because it makes the next leader nondeterministic and
  can select a different voter.

## Related Changes

- None.

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
