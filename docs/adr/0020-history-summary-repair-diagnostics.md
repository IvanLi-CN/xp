# ADR 0020: Persisted Summary Repair Diagnostics

## Status

Accepted

## Context

After recovery generation 2 was consumed, node 101 could retain a bounded set of summary segment IDs
while the lifecycle worker continued to report `syncing`. The durable checkpoint showed the pending
set, but it did not say whether the latest attempt failed in transport, observed a changed response,
made no progress, or failed while committing SQLite state. Operators therefore could not distinguish
a retryable public HTTPS outage from a malformed or oversized repair response.

## Decision

Resume the existing consumed-generation checkpoint in place and persist additive summary-repair
diagnostics in the existing control snapshot. Each peer diagnostic records the pending count,
summary cursors, attempt and progress times, result category, error code, retry deadline, and
bounded attempt count. The categories are `progress`, `explicit_unavailable`, `response_changed`,
`no_progress`, `transport_error`, `storage_error`, and `in_progress`.

The repair worker records an attempt before the network call. A public HTTPS transport failure keeps
the checkpoint unchanged and schedules a bounded retry. A changed response identity clears only that
identity and retries the same pending set. A response that explicitly marks IDs unavailable removes
only those IDs. A response that cannot advance leaves all pending IDs in place and reports
`no_progress`. A stored segment that cannot fit the 256 KiB response budget returns a storage error
before producing a response identity or changing checkpoint state.

Diagnostic fields are additive and defaulted so older snapshots and peers remain compatible. The
existing summary, gap, watermark, response identity and generation consumption updates remain
atomic;
diagnostics never authorize a source ACK, retention crossing, or history deletion. Permanent gaps
remain visible, queries remain `partial`, and `replica_converged` remains false.

## Consequences

Operators can identify the blocker from read-only runtime status and correlate it with public HTTPS
health and SQLite diagnostics. A no-progress condition remains safely retryable instead of being
mistaken for completion. The control snapshot grows by a bounded per-peer record and remains subject
to the existing state-size guard. No extra database, shadow copy, quota change, full `VACUUM`, or
source-outbox deletion is introduced.

## Validation

Unit coverage verifies diagnostic category mapping, pending checkpoint and diagnostic persistence
across restart, old defaulted state compatibility, and the response wire-budget guard. Release
validation still requires the 128 MiB/no-swap resource run and live 101 summary catch-up, Ready
stability, and deep verification after merge.
