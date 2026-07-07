# Raft Membership Voter Invariant History

## Key Decisions

- 2026-07-07: Stable membership nodes are all voters. Long-lived learners and configurable voting
  rights are unsupported.
- 2026-07-07: Join success is defined as voter success; asynchronous best-effort promotion is not a
  valid API success condition.
- 2026-07-07: No-quorum repair remains an explicit disaster recovery operation, not an automatic
  disk rewrite.
