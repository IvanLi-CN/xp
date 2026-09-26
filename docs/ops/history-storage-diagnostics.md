# History storage stall diagnostics

XP keeps a small node-local diagnostic record for the SQLite operations that can explain a
repository status or synchronization stall. The record is intended for an operator investigating an
incident on a host-managed or container node. It is not an API, a health signal, or a replacement
for the history database.

## Record

The file is `${XP_DATA_DIR}/history.sqlite3.diagnostics.json`.

- The file is private (`0600`) and atomically replaced.
- The serialized state is capped at 16 KiB and retains only the current in-flight operation, the
  last operation slower than one second, the last operation interrupted across a process restart,
  and bounded completion counters.
- Each event has a stable `operation_id`, a static `caller_class`, a static SQL template or
  composite-operation description, wall-clock start and finish times, monotonic `elapsed_ms`, an
  outcome, and an optional result count.
- SQL templates retain placeholders such as `?1`; they never include bind values. History rows,
  payloads, tokens, request bodies, and URLs are not recorded.

The instrumented boundaries are:

- repository runtime status and its metadata-only record and segment counts;
- tiered backfill page execution, export leases, received-at cutoff, and both watermark reads;
- retention expiry probes, compaction pages, export leases, and replacement/prune maintenance.

An operation writes its in-flight description before the blocking boundary. On normal completion it
writes the elapsed result. If the process is restarted while an operation is in flight, the next
startup moves that description to `last_interrupted_event`. A failed operation is retained with its
scope-exit outcome when the guard is unwound.

## Incident collection

Preserve the diagnostic file before restarting or upgrading the affected node. Read it from the
node's configured data directory with an operator-approved read-only method, then correlate
`started_at_unix_ms`, `finished_at_unix_ms`, and `elapsed_ms` with the node's `/api/health`,
resource monitoring data, and host I/O or PSI observations. The diagnostic event identifies the
SQLite boundary; the health and host records establish whether that operation coincided with service
unavailability or storage pressure.

On a host-managed node, the read-only collection is equivalent to:

```text
sudo -n cat ${XP_DATA_DIR}/history.sqlite3.diagnostics.json
```

Use the node's actual configured `XP_DATA_DIR`; do not substitute a guessed data directory.

Do not delete `history.sqlite3`, its WAL, or the diagnostic file while collecting evidence. The
diagnostic file is safe to omit from a normal incident report if it contains no in-flight,
interrupted, or slow event; that is an evidence gap, not proof that SQLite was healthy.

## Acceptance checks

The implementation must pass these focused checks before publication:

```text
cargo fmt -- --check
cargo test --lib history_storage
cargo test --lib history_repository::replica::runtime
```

The tests verify the bounded JSON shape, private-operation redaction, restart recovery, the real
record-count path, and the existing history repository synchronization and retention semantics.
The instrumentation does not alter API responses, Raft state, history rows, cursors, leases,
retention decisions, or synchronization behavior.
