# ADR 0022: Signed Retained Stream Boundary for Consumed Repair

## Status

Accepted

## Context

The v3.43.14 continuation path correctly rejects a consumed-generation repair response when the
receiver has no previously recorded transient gap. In production, a delayed summary retry can
still encounter a source whose retention window has moved past the receiver watermark.

That leaves a new exact boundary gap while the same pending response remains valid.

## Decision

Repair responses from a truncated source may carry a bounded `retained_stream_boundaries` list.
Each entry is derived from the source's durable segment rows and declares the minimum retained
`first_sequence` for one `(source_node_id, source_epoch, stream)`.

The boundary metadata is included in a separate content digest while the legacy `response_id`
remains unchanged for v1 peers.

A consumed-generation receiver may seal a new range only when the response is authenticated over
public HTTPS, the stream matches the consumed recovery handoff, and the first returned segment
equals the declared retained boundary.

The segment must also have a predecessor hash. The receiver then commits the permanent
`source_retention_expired` gap, anchor, watermark and response checkpoint atomically. Missing or
malformed boundary metadata remains fail-closed; old peers continue the legacy response identity
and cannot authorize the continuation.

## Consequences

This completes delayed consumed-generation summary repair without re-arming a generation, creating
a database copy, changing source epoch, or altering ordinary delivery and anti-entropy continuity.

A source that cannot prove its current retained start leaves the pending page intact and reports
the existing storage-boundary diagnostic. Permanent gaps continue to make queries `partial` and
keep `replica_converged=false`.

## Validation

Tests cover legacy response identity fallback, boundary digest binding, exact boundary acceptance
without a transient gap, mismatched boundaries, and bounded repair serialization.

Resource validation and live 101 summary, Ready stability and deep verification remain delivery
gates.
