---
title: Fresh join admission ignores unrelated learners
module: cluster membership lifecycle
problem_type: over-broad admission precondition
component: fresh join admission and legacy session migration
tags: [raft, learner, fresh-join, membership, lifecycle]
status: active
related_specs:
  - 7mvqp-raft-membership-voter-invariant
  - 38wmj-cluster-node-onboarding
---

# Fresh Join Admission Ignores Unrelated Learners

## Context

A cluster can retain a non-voter learner after an older join loses its coordinator state. The
learner is an observable replication incident, but it is not owned by a later fresh join request.
The new request creates its own reservation, learner, catch-up target, and promotion decision.

## Symptoms

- A healthy, quorum-backed cluster rejects a new node before it registers its learner.
- The rejection reports `unexpected_learners` for a different node.
- Operators are directed toward removing, repairing, or waiting on an unrelated learner merely to
  add capacity.

## Root Cause

The generic clean-membership guard was used as a fresh-join admission gate. That guard correctly
protects lifecycle operations that need a fully exact membership shape, but it incorrectly made
an unrelated non-voter incident a precondition for creating a new learner.

## Resolution

Use a fresh-join-specific guard after the all-voter lifecycle capability barrier. Require a
linearizable leader view, non-joint membership, a new target in both Raft and DesiredState, no
active membership operation, and valid voter-to-DesiredState mappings. Report unexpected learners
without treating them as a veto.

Keep catch-up and promotion scoped to the exact learner recorded by the new Join operation. For an
expired legacy `Reserved` session whose target remains a non-voter learner, terminalize only that
session with the existing additive state command. Retain the learner, DesiredState Node, and
endpoints for explicit recovery; do not issue a membership removal or synthesize a promotion.

## Guardrails

- Do not weaken the generic clean-membership guard used by delete, restore, eviction, or repair.
- Do not let a fresh join bypass voter quorum, joint-consensus, active-operation, identity, or
  voter/DesiredState mapping checks.
- Do not use learner reachability, lag, or health as an admission condition for another node.
- Do not automatically remove or promote a legacy learner while terminalizing its expired session.
- Treat joins while a voter is unavailable as a separate quorum and retained-voter capability
  problem; this rule does not relax that boundary.

## References

- `src/raft_membership_guard.rs`
- `src/join_coordinator.rs`
- `src/http/mod.rs`
- `docs/adr/0010-fresh-join-admission.md`
- `docs/specs/7mvqp-raft-membership-voter-invariant/SPEC.md`
- `docs/specs/38wmj-cluster-node-onboarding/SPEC.md`
