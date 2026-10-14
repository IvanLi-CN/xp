# ADR 0026: History Delivery Throughput Acceptance

## Status

Accepted

## Context

Production v3.43.21 is healthy and 101 is `ready`, but history delivery is not
converged. A 101 status sample moved from 929 pending segments at 18:41 CST to
973 at 19:27 CST. The ACK timestamp and oldest pending cursor advanced, yet the
pending queue grew. US moved from 2340 to 2210 pending segments in the same
period, but that short-term reduction does not prove sustained drain.

The current status surface exposes queue depth, bytes, oldest cursor, and the
last ACK time. The throughput implementation adds cumulative generated
segments/bytes, durable ACK segments/bytes, and repository-to-repository apply
counters. Therefore `delta(pending) / delta(time)` remains a net backlog
signal, while the persisted counters provide the processing-to-generation ratio.

`Ready`, `summary_repairs.pending_count=0`, and an ACK timestamp are separate
signals. They do not prove source delivery drain, retained-data digest equality,
or complete-history convergence. Permanent gaps must continue to produce
`partial` queries with `replica_converged=false`.

## Decision

The throughput target is a hard delivery gate:

1. For each five-minute window, durable processing capacity must be at least
   **10x the peak generated rate**, measured independently in segment/second and
   canonical-byte/second.
2. The target applies to both stages: source-to-repository durable ACK and
   repository-to-repository durable repair/anti-entropy apply.
3. The rate denominator is the highest observed sustained five-minute input
   rate for the relevant source workload. Aggregate stage capacity must meet the
   target while each stream remains sequence-strict and cannot be starved by a
   faster stream.
4. The calculation must use monotonic counters committed with the durable
   operation: enqueue counters for generated data, ACK counters for source
   delivery, and apply counters for repository-to-repository synchronization.
   Queue depth deltas and `last_acknowledged_at` remain diagnostic signals only. The metric row
   persists `peak_generated_segments` and `peak_generated_bytes` for the highest completed
   five-minute generated window in the current counter epoch; a later quiet window cannot weaken
   the denominator.
5. A release cannot pass the throughput gate until the counters, five-minute
   window calculation, and failure categories are observable through a read-only
   status surface and a deterministic red-capable test.

The implementation keeps source and repository workers on bounded five-second
ticks. A source tick drains at most sixteen local working pages; local receiver
commits use four segments with a 50ms yield between batches, while every page
remains below the one MiB wire budget and remote protocol pages retain the
256-segment/1 MiB contract. This bounds resident memory while removing the old
500ms local cadence that prevented the apply stage from maintaining 10x
headroom. Cadence does not permit unbounded concurrency, alter
sequence ordering, or bypass the public HTTPS path. Enqueue, ACK and receiver
checkpoint counters commit in the same SQLite transaction as their durable
mutation.

## Required evidence

- Per source and target: cumulative generated segments/bytes, durable ACK
  segments/bytes, repository apply segments/bytes, attempts, and categorized
  failures.
- Counter updates are idempotent across retries and restarts. Enqueue and ACK
  counters commit in the same SQLite transaction as their journal mutation;
  repository apply counters commit with the receiver checkpoint and watermark.
- A five-minute measurement records start/end counters, rates in both units,
  queue depth, oldest cursor, ACK age, response latency, and capacity state.
- Runtime status exposes the last completed durable 300-second window, including start/end, segment
  and byte deltas, peak generated deltas, milli-rates and independent source-ACK/repository-apply
  10x booleans. It also
  exposes cumulative source/stream/target scopes with their own completed/current windows so
  target starvation is observable. A status response with no completed window for a required
  scope is not a pass.
- A failing harness must assert the exact symptom: either stage below 10x in
  segments or bytes, a non-draining backlog, or a starved stream. It must run
  against the real delivery boundary and be fast enough for repeated diagnosis;
  the production gate remains a real-node five-minute observation.
- Deep verification remains independent: both repositories must return explicit
  v3 metadata with `partitions_included=true`, and all comparable retained
  sequence-block metadata must match. Permanent gaps still preserve `partial`
  and `replica_converged=false`.

## Ranked hypotheses and falsifiable probes

1. **Bounded page/scheduling budget is the bottleneck.** If true, a controlled
   increase in page concurrency or budget on the test workload raises durable
   ACK/apply throughput without violating the 128 MiB/no-swap and SQLite read
   limits.
2. **Public HTTPS latency or timeout is the bottleneck.** If true, the counter
   gap correlates with request latency, transport failures, or retries; a stable
   public HTTPS endpoint removes the gap without using Mesh or Reverse Mesh.
3. **SQLite commit or filesystem I/O is the bottleneck.** If true, commit time,
   iowait, or storage diagnostics rise with page size; a bounded transaction
   change improves both counters while preserving atomicity.
4. **Generation rate exceeds the observed drain window.** If true, cumulative
   generated counters exceed ACK/apply counters even when transport and storage
   remain healthy; freezing the producer in a controlled replay makes the queue
   drain at the measured processing rate.
5. **Summary digest differences are a separate retained-data issue.** If true,
   source delivery counters continue advancing while fixed v3 sequence-block
   differences remain unchanged. It must be debugged independently of backlog
   throughput.

## Constraints

The fix must continue the consumed generation-2 checkpoint in the existing
SQLite database and control snapshot. It must not re-arm recovery, cross another
retention boundary, copy or shadow the database, change quota, delete source
outbox rows, run full `VACUUM`, or use Mesh/Reverse Mesh for history traffic.

## Consequences

The existing status API now has additive durable counters used to build a bounded
rate view. Production evidence still requires a red-capable loop, Agent VM
resource validation, and a fresh acceptance window before any convergence claim.
