# ADR 0021: Consumed Summary Retention Continuation

## Status

Accepted

## Context

After recovery generation 2 was consumed, a failed retained-anchor attempt could leave an exact
non-permanent sequence gap in the receiver. A later repair response from the same bound stream
could carry `history_truncated=true` and the same anchor, but the worker rejected every retry
because the stream's recovery allowance was already marked consumed. The pending summary page then
reported `storage_error` forever even though the source had supplied the evidence needed to seal the
already observed range.

## Decision

Permit a consumed-generation continuation only when all of the following are true: the retry is for
the same bound source, epoch and stream; the checkpoint already contains the exact non-permanent
sequence gap produced by the failed anchor attempt; and the serving response explicitly carries
`history_truncated=true`. The receiver promotes that exact range to a permanent
`source_retention_expired` gap and accepts the supplied anchor in the same history transaction.

This path resumes the existing summary checkpoint. It never re-arms a generation, creates a new
tiered handoff, accepts an unobserved range, or changes the source epoch. Different or missing gap
evidence remains fail-closed and continues to report a non-complete summary.

## Consequences

The 101 repair worker can drain the pending page after a crash or failed first anchor attempt while
preserving `partial` and `replica_converged=false` semantics for the permanent gap. A malformed
response, a different range, or a SQLite write failure still leaves the durable checkpoint intact.

## Validation

Runtime coverage proves that an observed non-permanent gap is sealed exactly once and that a second
probe cannot reuse the consumed continuation. Focused history-repository tests and the Agent VM
resource gate remain required before publication.
