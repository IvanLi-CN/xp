# Retire a permanently decommissioned stale learner

## Context

The cluster can retain a DesiredState-mapped learner after a join attempt expires and the server is
permanently removed. The learner is not a voter and must not be promoted, but the normal node-delete
contract intentionally rejects learners and the stale-learner recovery path assumes the server may
catch up and become a voter.

## Decision

Add an operator-only, leader-local `retire-stale-learner` lifecycle. A zero-write preview proves one
exact unexpected learner, its DesiredState and Raft metadata mapping, the linearizable membership
fingerprint, and its endpoint snapshot. Apply requires the exact fingerprint and endpoint set,
requires `cluster.stale-learner-retirement-v1` from every current voter, and records the existing
durable `RemoveNode` operation with `remove_learner=true`.

The resumer issues exactly `RemoveNodes({target}, false)`, verifies that the learner is absent, and
then reuses the existing DesiredState deletion and runtime/history cleanup. A changed role,
membership fingerprint, endpoint set, leader, pending session, or abnormal membership shape blocks
the operation without guessing or editing Raft persistence.

## Alternatives

- Promote then delete: rejected because a decommissioned learner cannot prove catch-up and promotion
  would create an unnecessary voter transition.
- Reuse ordinary node deletion: rejected because its voter-only precondition protects the normal
  delete contract and cannot distinguish a stale learner from an in-flight join.
- Rewrite Raft membership files: rejected because it bypasses quorum-backed intent, compare-and-swap
  evidence, and uncertain-result recovery.
