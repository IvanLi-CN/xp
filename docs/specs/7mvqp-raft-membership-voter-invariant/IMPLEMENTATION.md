# Raft Membership Voter Invariant Implementation

## Current State

- Cluster join atomically reserves the token with the desired-state node, calls `add_learner`,
  records `learner_registered`, and returns bootstrap material without waiting for catch-up.
- The leader coordinator resumes durable pending sessions after restart or failover, waits for the
  required log index, and then calls `add_voters`. Expiry removes membership and node endpoints,
  then deletes the desired-state node while recording the terminal tombstone atomically.
- `raft_membership_guard` scans Raft metrics for `membership.nodes - voter_ids`.
- The guard promotes non-voter membership nodes only while running on the leader; non-leader states
  log the divergence and leave disk state untouched.
- Join/delete/coordinator membership transitions and guard repair share a membership operation gate.
  The guard excludes learners with pending join sessions and retains legacy learner repair for nodes
  without a session.
- Server startup spawns the guard with a periodic interval.

## Coverage

- HTTP join tests prove Phase 1 returns before catch-up/promotion and replays the same reservation.
- Guard tests cover non-voter detection, leader repair, in-flight operation skipping, and
  follower/no-leader non-repair.

## Remaining Gaps

- Production quorum recovery still requires an explicit owner-approved operation on the selected
  healthy node.
