# Recover a stale learner through the durable restore lifecycle

A learner without an active Join or Restore owner blocks lifecycle writes, but treating that shape
as permission for periodic auto-promotion can turn unrelated Raft metadata into a voter. The
supported recovery is therefore a leader-local, signed `xp-ops` command that proves one exact
DesiredState-mapped learner, requires a dry-run membership revision, records a normal Restore
operation, and then lets the existing restore resumer wait for catch-up and promote it.

## Considered options

- Resume automatic learner promotion: rejected because an audit finding alone cannot establish
  operator intent or protect unrelated abnormal members.
- Relax generic `restore-node`: rejected because its ordinary absent-node contract must not adopt
  a currently present learner.
- Rewrite Raft membership or snapshots: rejected because it bypasses durable intent,
  compare-and-swap evidence, and uncertain-result recovery.

## Consequences

The command accepts only one unique stale learner in an otherwise clean membership view, verifies
the exact DesiredState and Raft metadata mapping, checks every current voter for lifecycle
capability, and revalidates the preview fingerprint before its sole write. A matching
`learner_registered` JoinSession is terminally consumed in that same durable write; a `reserved`
session or any other pending session remains a blocking incident.
