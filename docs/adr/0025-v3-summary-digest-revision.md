# ADR 0025: Rebuild Existing v3 Summary Digests After Canonicalization

## Context

The v3 sequence block digest now removes the receiver-local `received_at_unix_seconds` field from
serialized `StoredRecord` wrappers. Existing SQLite blocks already stored v3 digests produced by
the predecessor algorithm, so changing only the rebuild function would leave those blocks clean
and publish stale hashes after upgrade.

## Decision

Persist a canonical payload digest revision in the repository control snapshot. A predecessor or
missing revision marks existing clean v3 blocks dirty once, then the bounded summary worker
rebuilds them with the current canonicalizer. Rebuilt rows retain protocol `summary_version=3` and
`digest_version=3`; the revision is only an internal migration marker.

## Consequences

The upgrade is in-place and restart-safe. It does not re-arm a recovery generation, copy the
database, change retention, or alter gap semantics. The first post-upgrade worker pass may expose
v3 summary metadata as unavailable until the dirty blocks are rebuilt; later restarts reuse the
persisted revision and do not repeat the full rebuild.
