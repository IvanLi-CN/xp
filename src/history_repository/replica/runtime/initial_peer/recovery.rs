use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn prune_initial_peer_backfill_checkpoints(
        &mut self,
        active_peer_node_ids: &BTreeSet<String>,
    ) -> Result<usize, RepositoryRuntimeError> {
        let stale_peer_node_ids = self
            .snapshot
            .initial_peer_backfills
            .keys()
            .filter(|peer_node_id| !active_peer_node_ids.contains(*peer_node_id))
            .cloned()
            .collect::<Vec<_>>();
        if stale_peer_node_ids.is_empty() {
            return Ok(0);
        }
        for peer_node_id in &stale_peer_node_ids {
            self.snapshot.initial_peer_backfills.remove(peer_node_id);
        }
        self.persist_control_state()?;
        Ok(stale_peer_node_ids.len())
    }

    pub(crate) fn resume_legacy_consumed_summary_page(
        &mut self,
        peer_node_id: &str,
        now_unix_seconds: u64,
    ) -> Result<bool, RepositoryRuntimeError> {
        let Some(checkpoint) = self.snapshot.initial_peer_backfills.get(peer_node_id) else {
            return Ok(false);
        };
        if !checkpoint.recovery_generation_consumed
            || checkpoint.summary_pending_segment_ids.len() != MAX_REPAIR_SEGMENTS
            || checkpoint.summary_pending_revisit_cursor
            || checkpoint.summary_pending_next_cursor == checkpoint.summary_cursor
        {
            return Ok(false);
        }

        let previous_snapshot = self.snapshot.clone();
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .get_mut(peer_node_id)
            .expect("validated legacy summary checkpoint");
        checkpoint.summary_pending_next_cursor = checkpoint.summary_cursor.clone();
        checkpoint.summary_pending_revisit_cursor = true;
        checkpoint.summary_complete = false;
        self.record_summary_repair_result_in_memory(
            peer_node_id,
            now_unix_seconds,
            SummaryRepairResult::Progress,
            None,
            None,
        );
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(true)
    }

    /// Seal the exact gap left by a failed consumed-generation anchor attempt when the source
    /// later proves that its retained history is truncated.
    pub(crate) fn can_accept_consumed_retained_sequence_gap(
        &self,
        wire: &[u8],
        retained_first_sequence: Option<u64>,
    ) -> Result<bool, RepositoryRuntimeError> {
        let segment = crate::history_sync::SignedSegment::from_wire(wire)?;
        let first = segment.canonical().first_cursor();
        let Some(receiver) = self.receiver.as_ref() else {
            return Ok(false);
        };
        let Some(watermark) = receiver.continuous_watermark(first)? else {
            return Ok(false);
        };
        let Some(expected) = watermark.sequence().checked_add(1) else {
            return Ok(false);
        };
        if first.sequence() <= expected || segment.canonical().previous_segment_hash().is_none() {
            return Ok(false);
        }
        if retained_first_sequence != Some(first.sequence()) {
            return Ok(false);
        }
        let matching_gap = self.snapshot.gaps.iter().filter(|gap| {
            !gap.permanent
                && gap.source_node_id == first.source_node_id()
                && gap.source_epoch == first.source_epoch()
                && gap.stream == first.stream()
                && gap.first_sequence == expected
                && gap.last_sequence == first.sequence().saturating_sub(1)
        });
        let matching_count = matching_gap.count();
        let transient_gap_count = self
            .snapshot
            .gaps
            .iter()
            .filter(|gap| {
                !gap.permanent
                    && gap.source_node_id == first.source_node_id()
                    && gap.source_epoch == first.source_epoch()
                    && gap.stream == first.stream()
            })
            .count();
        Ok((matching_count == 1 && transient_gap_count == 1)
            || (matching_count == 0 && transient_gap_count == 0))
    }
}
