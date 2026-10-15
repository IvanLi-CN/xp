# ADR 0027: Bridge Existing Retention Gaps During Source Delivery

## Status

Accepted

## Context

An ordinary source stream can resume at the first retained sequence after a durable
`source_retention_expired` gap. The receiver already records that range, but a source delivery page
previously emitted only rotating local backpressure gaps. The receiver then rejected the signed
anchor with `HashChainMismatch` on every retry, leaving the durable outbox and ACK cursor unchanged.

## Decision

When a bounded source page has pending segments, the source may include existing permanent gaps
whose source node, epoch, stream, reason (`source_retention_expired`), and `last_sequence + 1`
exactly equal a later segment's cursor. The page keeps tombstone-first ordering; gap metadata is
attached to the first request and merged before the page is applied, so the matching later segment
advances that declared range once during the same ordered page. The gap remains in the durable
ledger, no new gap is created, and no gap is deleted or rewritten. The source epoch, payload,
outbox, recovery generation, and public HTTPS route are unchanged.

The retained anchor consumes one slot from the existing 64-entry gap page budget. Page construction
stops before a 65th distinct retained or required backpressure evidence entry, while ordinary
segments that consume no evidence keep the 256-segment page bound. Other gaps continue to use the
persisted rotating cursor. A retry after a durable segment commit is an idempotent
duplicate; a retry after a failed transaction restores the prior checkpoint and can cross the same
declared range again. All later segments and all non-adjacent ranges remain subject to ordinary
sequence and hash-chain validation.

## Consequences

The first post-retention source page can make progress without re-arming recovery or changing
permanent-history semantics. A valid retained gap remains visible in summaries, queries remain
`partial`, and `replica_converged` remains false while any permanent gap exists. A malformed,
changed,
or non-adjacent gap still fails closed and leaves the pending checkpoint intact.
