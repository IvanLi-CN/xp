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

    pub(crate) fn derived_maintenance_deferred(&self) -> bool {
        self.historical_ingest_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
    }

    pub(super) fn note_historical_ingest<'a>(
        &mut self,
        wires: impl IntoIterator<Item = &'a [u8]>,
        now: u64,
    ) {
        // Only a committed, verified historical page can renew this process-local window.
        // Live traffic cannot postpone metadata forever; quiet/restart clears the priority.
        if wires.into_iter().any(|wire| {
            SignedSegment::from_wire(wire).is_ok_and(|segment| {
                segment
                    .canonical()
                    .closed_at_unix_seconds()
                    .saturating_add(15 * 60)
                    < now
            })
        }) {
            self.historical_ingest_at = Some(Instant::now());
        }
    }
}
