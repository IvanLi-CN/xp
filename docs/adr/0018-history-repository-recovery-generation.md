# ADR 0018: History Repository Recovery Generations

## Context

An initial peer backfill can retain a completed tiered handoff after the bounded gap ledger rotates.
A later repair page may expose a newer retained anchor while the receiver watermark is still before
the old handoff. Treating any historical marker as a duplicate blocks recovery; treating it as
permission would allow an unbounded second retention crossing.

The retained history also needs a deep-verification summary that survives late rows, tombstones,
retention deletes, restart, and continuous writes without rebuilding the SQLite table or creating a
shadow database.

## Decision

Recovery crosses one retention boundary only through an explicit, signed local operator command. The
command computes a deterministic fingerprint from the peer checkpoint, receiver watermark, prior
handoff evidence, capacity preflight, and recovery generation. Dry-run has no writes. Apply requires
`--yes` and the expected fingerprint; the checkpoint is atomically armed before the worker can
consume the generation. A generation is consumed once and cannot be reused. The source epoch,
ordinary source delivery, anti-entropy ordering, and retained permanent gaps remain unchanged.

Recovery updates the existing `${XP_DATA_DIR}/history.sqlite3` and control snapshot in place. It
never creates a second history database, copies the database, deletes source outbox rows, expands
quota, or runs full `VACUUM`. Capacity and free-space checks fail closed before history writes.

Deep verification uses additive SQLite sequence-block Merkle metadata with a fixed 4096-sequence
block. Block metadata is rebuilt in resumable bounded pages and invalidated by record, tombstone,
and retention mutations. A v2 capability is advertised only after metadata is complete. Peers
without the capability continue using the existing partition summary and never receive a v2-only
readiness requirement.

Permanent gaps remain durable evidence. Ready status may complete bounded catch-up, but
`replica_converged` remains false and queries remain `partial` while a permanent gap or incomplete
summary exists.

## Consequences

The 101 recovery is auditable and idempotent: a stale marker can be classified, armed once, replayed
after a crash, and observed through the existing ready stability window. Operators must run the
local
recovery command during the approved maintenance window after the release is deployed. A missing
source range remains a real gap and is not silently rewritten as complete history.
