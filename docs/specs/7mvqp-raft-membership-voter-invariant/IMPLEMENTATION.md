# Raft Membership Voter Invariant Implementation

## Current State

- Cluster join calls `add_learner`, writes the node into desired state, waits for learner catch-up,
  then synchronously calls `add_voters` before returning success.
- If learner catch-up or voter promotion fails, join returns an internal error and best-effort
  rollback removes the just-added learner and state node.
- `raft_membership_guard` scans Raft metrics for `membership.nodes - voter_ids`.
- The guard promotes non-voter membership nodes only while running on the leader; non-leader states
  log the divergence and leave disk state untouched.
- Server startup spawns the guard with a periodic interval.

## Coverage

- HTTP join tests cover `add_voters` failure and state rollback.
- Guard tests cover non-voter detection, leader repair, and follower/no-leader non-repair.

## Remaining Gaps

- Production quorum recovery still requires an explicit owner-approved operation on the selected
  healthy node.
