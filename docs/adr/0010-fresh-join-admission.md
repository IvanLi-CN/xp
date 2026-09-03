# Admit fresh joins independently of unrelated learners

## Status

Accepted

## Context

OpenRaft exposes learners as replication-only members. A learner can remain in membership after a
join process loses its coordinator state, while the current voters and the leader remain healthy.
Treating that incident as a global clean-membership failure makes an unrelated fresh join depend on
the recovery of a non-voter. It also gives the fresh join path an accidental reason to inspect or
mutate a learner that it does not own.

The learner that a fresh join creates still needs strict catch-up and promotion checks. Those checks
are meaningful only for the exact target recorded by that Join operation.

## Decision

Use a fresh-join-specific admission gate. After the existing all-voter capability barrier and a
linearizable leader check, it blocks only on:

- joint consensus;
- an active membership operation;
- the requested identity already being present in Raft or DesiredState; and
- voter/DesiredState mapping invariants other than the `unexpected_learners` category.

The generic clean-membership gate remains unchanged for deletion, restore, eviction, and repair
operations. It continues to report unexpected learners and reject safety-sensitive operations that
need an exact membership shape. Fresh join logs unexpected learners for incident visibility but
does not use their health, lag, or availability as an admission condition.

When migration encounters an expired legacy `Reserved` JoinSession whose exact target is still a
non-voter learner, it records the session as `Expired` using the existing additive `UpsertNode`
command. It retains the learner and DesiredState Node and does not create a cleanup operation or
issue a membership removal. An active recorded Join operation keeps its existing expiry cleanup
behavior.

## Consequences

Fresh node expansion can proceed while an unrelated learner is stale or offline, provided the
leader still has a quorum-backed linearizable view and the requested identity is new. The stale
learner remains visible for explicit operator recovery and is never promoted or deleted by this
path. The existing capability barrier and the single active-operation serialization still limit
which cluster-wide changes may start.

This decision intentionally does not enable membership changes when a voter is unavailable. That is
a separate quorum and retained-voter capability problem with different safety and operator-consent
requirements.
