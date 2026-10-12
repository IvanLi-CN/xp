# ADR 0024: Canonical v3 Summary Payloads

Status: accepted

## Context

SQLite history rows retain a serialized `StoredRecord` wrapper so existing readers can recover the
typed record. The wrapper includes the repository-local receive timestamp. Two repositories can
therefore hold identical source history with different wrapper bytes, which would make a sequence
block Merkle digest depend on where the record arrived.

## Decision

During bounded v3 sequence-block rebuilds, normalize only payloads whose source, epoch, stream, and
sequence match the SQLite metadata and whose wrapper carries `received_at_unix_seconds`. Remove
that field before hashing. Arbitrary payload bytes and legacy rows that do not match the wrapper
shape remain hashable with their original bytes. Rebuilding stays in the existing SQLite database;
no history rows are rewritten and v1 fallback remains unchanged.

## Consequences

v3 digests compare retained source history across repositories while ignoring local arrival time.
Existing v3 metadata is rebuilt in place through the existing dirty-block mechanism. Permanent gaps
continue to keep queries partial and `replica_converged=false`.
