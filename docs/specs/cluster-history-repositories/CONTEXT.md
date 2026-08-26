# Cluster History Repositories

This glossary defines the domain language for long-term cluster history. It
distinguishes temporarily undelivered history from history that no authorised
source or repository can still provide.

## Replication

**History Repository**:
A configured cluster node that holds a durable replica of the cluster history.
_Avoid_: archive node, data warehouse

**Ready History Repository**:
A History Repository that can serve the complete known history union after its
bounded catch-up and stability window. It may still expose a known Permanent
Gap as partial history.
_Avoid_: fully converged repository, error-free repository

**Replica Convergence**:
The condition in which a History Repository has no repairable difference and
no Permanent Gap relative to the known history union.
_Avoid_: ready, available

**Source**:
A cluster node that authors an ordered, signed history stream under its own
identity.
_Avoid_: sender, producer

**Source Cursor**:
The immutable position of an item in a Source stream, identified by source,
epoch, stream, and sequence.
_Avoid_: timestamp, offset

**Collector**:
The selected History Repository that durably accepts a Source segment and
acknowledges its continuous Source Cursor watermark.
_Avoid_: relay, proxy

**Source Delivery Journal**:
The durable source-owned record of signed segments awaiting a Collector
acknowledgement. It is a transfer obligation, not ordinary node history or a
repository replica.
_Avoid_: in-memory outbox, history retention

**Recoverable Backlog**:
Source history that has not yet reached a Collector but remains available from
the Source or a History Repository. It is not a gap.
_Avoid_: temporary gap, pending loss

**Source Capture Suspension**:
The explicit incomplete state in which a Source cannot safely append to its
Source Delivery Journal. It has no Source Cursor and does not claim that
unobserved history was synchronized.
_Avoid_: permanent gap, successful empty interval

**Repair**:
The cursor-preserving transfer of retained history from a Source or a ready
History Repository that fills a Recoverable Backlog or a missing replica range.
_Avoid_: replay from scratch, resynchronization

**Permanent Gap**:
An explicit Source Cursor range whose original history has expired under the
unchanged source-retention policy and cannot be supplied by its Source or any
ready History Repository. A delivery failure or a full in-memory outbox is
never a Permanent Gap.
_Avoid_: backpressure gap, temporary gap
