# ADR 0023: Legacy Consumed Summary Page Revisit

## Status

Accepted

## Context

Before bounded summary pages were made resumable, a consumed recovery checkpoint could retain 64
pending segment IDs while storing the next page cursor. The receiver then rejected the retained
anchor safely, but never reread the page that still contained the omitted IDs.

## Decision

At the start of a ready-peer catch-up tick, detect only the durable legacy shape: a consumed
generation, exactly one bounded repair batch, a different pending-next cursor, and no existing
revisit marker. Persist an in-place revisit marker and point the pending cursor back to the current
summary cursor. The next retry drains the same page under the current bounded-page rules.

The migration does not re-arm recovery, change the source binding or receiver watermark, create a
new handoff, alter the source outbox, or remove a gap. Any other checkpoint shape remains subject
to the normal fail-closed validation.

## Consequences

Old production checkpoints can resume without operator database edits. The migration is durable and
idempotent across restart, and permanent retention gaps continue to make queries partial with
`replica_converged=false`.

## Validation

Runtime tests cover persistence, generation-consumed preservation, idempotent detection and the
bounded ready-peer repair path. Production validation must still observe pending IDs drain, the
Ready stability window and v3 deep verification after rollout.
