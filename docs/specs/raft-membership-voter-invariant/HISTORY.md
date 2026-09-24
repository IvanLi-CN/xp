# Raft membership lifecycle decisions

- `voter`, `learner`, and `absent` are role outcomes, not configurable classes. "Non-voter" is
  only a set description and must not drive promotion.
- A periodic scan is evidence and recovery scheduling, not an authority to decide a membership
  transition.
- Every membership mutation begins from a durable intent and a linearizable membership fingerprint.
  Unknown outcomes are resumed only when the observed shape is an exact current or next state.
- Repairing a known orphan voter is narrower than general disaster recovery: it removes one verified
  non-leader voter with retain=false and never rewrites local Raft files or desired/user data.
- Mixed-version voters cannot safely decode the new state-machine command variants. Lifecycle work
  freezes until the whole voter set advertises the capability.
- Capability verification follows the signed Mesh control-plane path. Only a predecessor's
  unacknowledged `404` for that signed route falls back to the legacy public capability endpoint;
  other missing or invalid acknowledgements remain terminal. Every response body stays within the
  same probe budget and a 64 KiB limit. A locally previewed unique orphan is excluded only from
  its own repair preflight, so stale public metadata cannot weaken the retained-voter upgrade
  barrier.
- A Stale Learner is recovered by explicitly adopting only the proven learner into the existing
  Restore lifecycle. It is distinct from an absent-node restore and from a pending Join session;
  automatic promotion remains forbidden.
- A permanently decommissioned Stale Learner is retired through a separate explicit lifecycle that
  removes the exact learner with `RemoveNodes(..., false)` before deleting its DesiredState data;
  recovery and promotion remain distinct operations.
- Fresh joins are admitted independently of unrelated stale learners. An expired legacy Reserved
  session keeps its learner and DesiredState Node while only the session is terminalized.
