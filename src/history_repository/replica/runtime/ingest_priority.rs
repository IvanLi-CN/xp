use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn prepare_for_replication(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        if self.derived_maintenance_deferred() {
            return Ok(());
        }
        self.advance_partition_summary_rebuild_page()?;
        self.advance_sequence_summary_block_rebuild_slice(1)?;
        self.rebuild_if_stale(now_unix_seconds)?;
        self.prune_retention(now_unix_seconds)
    }

    pub(crate) fn derived_maintenance_deferred(&mut self) -> bool {
        if self
            .historical_ingest_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
        {
            return true;
        }
        // Consume the expired window before a bounded maintenance attempt. New historical
        // commits may start the next window, but cannot postpone this attempt indefinitely.
        self.historical_ingest_at = None;
        false
    }

    pub(super) fn note_historical_ingest<'a>(
        &mut self,
        wires: impl IntoIterator<Item = &'a [u8]>,
        now: u64,
    ) {
        // Only a committed historical page opens a window. Keep its original deadline
        // until maintenance consumes it, even when more historical pages keep arriving.
        if self.historical_ingest_at.is_none()
            && wires.into_iter().any(|wire| {
                SignedSegment::from_wire(wire).is_ok_and(|segment| {
                    segment
                        .canonical()
                        .closed_at_unix_seconds()
                        .saturating_add(15 * 60)
                        < now
                })
            })
        {
            self.historical_ingest_at = Some(Instant::now());
        }
    }
}
