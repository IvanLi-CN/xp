use super::*;

#[derive(Default)]
pub(super) struct RepositorySchedulingState {
    pub(super) historical_ingest_at: Option<Instant>,
    initial_sources_attempted: BTreeSet<String>,
}

impl RepositoryReplicaRuntime {
    pub(crate) fn select_initial_history_source_peer_ids(
        &mut self,
        peer_progress: Vec<(String, u64)>,
        local_node_id: &str,
        limit: usize,
    ) -> Vec<String> {
        let mut peers = peer_progress
            .into_iter()
            .filter(|(node_id, _)| node_id != local_node_id)
            .collect::<Vec<_>>();
        let eligible = peers
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<BTreeSet<_>>();
        let attempted = &mut self.scheduling.initial_sources_attempted;
        attempted.retain(|id| eligible.contains(id));
        if attempted.len() == eligible.len() {
            attempted.clear();
        }
        peers.sort_by(|(left_id, left_progress), (right_id, right_progress)| {
            left_progress
                .cmp(right_progress)
                .then_with(|| left_id.cmp(right_id))
        });
        // Each pending source gets one attempt per sweep, including failures. Progress
        // orders a sweep but cannot let unavailable low-progress peers monopolize it.
        // This process-local schedule never advances a durable checkpoint or ACK.
        peers
            .into_iter()
            .filter(|(id, _)| !attempted.contains(id))
            .take(limit)
            .map(|(id, _)| id)
            .collect()
    }

    pub(crate) fn note_initial_history_source_attempt(&mut self, node_id: &str) {
        self.scheduling
            .initial_sources_attempted
            .insert(node_id.to_owned());
    }

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
            .scheduling
            .historical_ingest_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
        {
            return true;
        }
        // Consume the expired window before a bounded maintenance attempt. New historical
        // commits may start the next window, but cannot postpone this attempt indefinitely.
        self.scheduling.historical_ingest_at = None;
        false
    }

    pub(super) fn note_historical_ingest<'a>(
        &mut self,
        wires: impl IntoIterator<Item = &'a [u8]>,
        now: u64,
    ) {
        // Only a committed historical page opens a window. Keep its original deadline
        // until maintenance consumes it, even when more historical pages keep arriving.
        if self.scheduling.historical_ingest_at.is_none()
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
            self.scheduling.historical_ingest_at = Some(Instant::now());
        }
    }
}
