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
- Capability verification follows the supported Mesh control-plane path before public fallback. A
  locally previewed unique orphan is excluded only from its own repair preflight, so stale public
  metadata cannot weaken the retained-voter upgrade barrier.
