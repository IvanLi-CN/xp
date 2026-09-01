# Use immutable monitor revisions and slotted execution

Service Monitor edits create an immutable Monitor Revision that becomes active
at the next UTC-aligned Schedule Slot. Existing Observations retain the revision
that produced them. After a restart, the scheduler executes only the current
and future slots; missed slots remain uncovered and are not backfilled.

This preserves the meaning of historical measurements when targets or check
criteria change, and prevents restart recovery from creating a probe burst or
rewriting past coverage. A deleted monitor stops future slots but does not erase
retained history; recreating the same target creates a new monitor identity.
