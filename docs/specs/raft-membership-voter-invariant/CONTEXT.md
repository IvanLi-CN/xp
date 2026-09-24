# Raft Membership Lifecycle

This glossary distinguishes the valid Raft member roles from abnormal membership shapes and
the narrowly scoped operator actions that recover them.

## Roles

**Voter**:
A Raft member that may vote and has exactly one DesiredState Node mapping. It is the only valid
steady-state membership role.
_Avoid_: member, normal node.

**Learner**:
A replication-only Raft member. During a normal lifecycle it is owned by one active Join or Restore
operation while that operation is in its learner phase. If the owner is missing, it is a stale
learner incident: it remains observable and is not promoted or deleted by periodic work. Its
presence does not block an unrelated fresh join.
_Avoid_: non-voter member, standby voter.

**Stale Learner**:
A learner with an exact DesiredState Node mapping but no active operation that owns its transition.
It is an incident shape, not evidence that periodic work may promote it. A legacy expired
`Reserved` session for this learner may be terminalized without changing the learner or Node; any
later recovery is an explicit operation.
_Avoid_: recoverable member, pending voter.

**Absent**:
An identity that is not present in Raft membership. It is the valid outcome of a completed node
removal or an input to ordinary restore.

## Recovery

**Membership Revision**:
An opaque fingerprint of the linearizable Raft membership view. It binds an operator's dry-run
preview to one later apply request.
_Avoid_: version, generation.

**Stale Learner Recovery**:
An operator-requested adoption of one proven Stale Learner into a durable Restore operation. It
does not grant periodic work a general promotion authority.
_Avoid_: auto-promotion, Raft repair.
