# History storage stall diagnostics

XP keeps a small node-local diagnostic record for the SQLite operations that can explain a
repository status or synchronization stall. The record is intended for an operator investigating an
incident on a host-managed or container node. It is not an API, a health signal, or a replacement
for the history database.

## Record

The file is `${XP_DATA_DIR}/history.sqlite3.diagnostics.json`.

- The file is private (`0600`) and atomically replaced.
- The serialized state is capped at 16 KiB and retains only the current in-flight operation, the
  latest slow operation, the latest slow leaf SQL operation, the last operation interrupted across
  a process restart, and bounded completion counters.
- Each event has a stable `operation_id`, a static `caller_class`, a static SQL template or
  composite-operation description, wall-clock start and finish times, monotonic `elapsed_ms`, an
  outcome, and an optional result count.
- SQL templates retain placeholders such as `?1`; they never include bind values. History rows,
  payloads, tokens, request bodies, and URLs are not recorded. `last_slow_leaf_event` preserves the
  most specific SQL boundary when a composite operation finishes after it.

The instrumented boundaries are:

- repository runtime status and its metadata-only record and segment counts;
- tiered backfill page execution, export lease refresh/finish/session checks, received-at cutoff,
  and both watermark reads;
- retention expiry probes, compaction pages, export leases, and replacement/prune maintenance.
- source-delivery journal summary reads used by runtime status.

An operation records its in-flight description in memory before the blocking boundary and queues it
to one dedicated, coalescing writer. The writer is rate-limited to one atomic replacement per 100 ms
and runs outside SQLite/backend locks, so diagnostics do not add a synchronous fsync to each query.
On normal completion it writes the elapsed result. If the process is restarted while an operation is
in flight, the next startup moves that description to `last_interrupted_event`. A failed operation
is retained with its scope-exit outcome when the guard is unwound. The file is evidence, not a
transactional health signal; collect it before restart because an abrupt process or host failure may
leave the last queued update unwritten.

## Incident collection

Preserve the diagnostic file before restarting or upgrading the affected node. Read it from the
node's configured data directory with an operator-approved read-only method, then correlate
`started_at_unix_ms`, `finished_at_unix_ms`, and `elapsed_ms` with the node's `/api/health`,
resource monitoring data, and host I/O or PSI observations. The diagnostic event identifies the
SQLite boundary; the health and host records establish whether that operation coincided with service
unavailability or storage pressure.

For the generated host-managed assets, collect the diagnostic, health, and host I/O evidence before
restarting. The commands are read-only:

```sh
# OpenRC and systemd generated assets use /var/lib/xp/data.
sudo -n date -u
sudo -n cat /proc/pressure/io
sudo -n cat /proc/loadavg
sudo -n cat /var/lib/xp/data/history.sqlite3.diagnostics.json
sudo -n ls -l /var/lib/xp/data/resource_metrics.sqlite3*
curl --max-time 5 -fsS http://127.0.0.1:62416/api/health

# With the operator's already-authorized admin token, capture current and I/O history.
curl --max-time 5 -fsS \
  -H "Authorization: Bearer ${XP_ADMIN_TOKEN}" \
  http://127.0.0.1:62416/api/admin/nodes/<node-id>/resources
curl --max-time 5 -fsS \
  -H "Authorization: Bearer ${XP_ADMIN_TOKEN}" \
  'http://127.0.0.1:62416/api/admin/nodes/<node-id>/resources/history?metric=cpu_iowait_percent'\
  '&limit=1500'

# Docker Compose uses the mounted path inside the official container.
docker compose exec -T xp cat /proc/pressure/io
docker compose exec -T xp cat /var/lib/xp/data/history.sqlite3.diagnostics.json
docker compose exec -T xp ls -l /var/lib/xp/data/resource_metrics.sqlite3*
```

Use the node's actual configured `XP_DATA_DIR` when it differs from the generated default; do not
substitute a guessed data directory. Also preserve the matching `history.sqlite3-wal` and
`history.sqlite3-shm` files, plus the `resource_metrics.sqlite3-wal` and
`resource_metrics.sqlite3-shm` files during collection. If the container is stopped, use its
read-only mounted volume path instead of `docker compose exec`:

```sh
docker compose run --rm --no-deps \
  --entrypoint ls xp -l /var/lib/xp/data
```

Do not start XP solely to collect evidence.

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
