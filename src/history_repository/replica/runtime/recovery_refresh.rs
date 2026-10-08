use super::super::ReplicaError;
use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn refresh_initial_peer_recovery_pending(
        &mut self,
        expected: &InitialPeerBackfillCheckpoint,
        peer_node_id: &str,
        pending: Vec<String>,
        gaps: &[RepositoryReplicaGap],
    ) -> Result<(), RepositoryRuntimeError> {
        let current = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .ok_or(ReplicaError::InvalidIdentifier)?;
        if current.recovery_generation == 0
            || current.recovery_generation_consumed
            || current.recovery_generation != expected.recovery_generation
            || current.summary_tiered_handoff.is_some()
            || current.summary_cursor != expected.summary_cursor
            || current.summary_pending_segment_ids != expected.summary_pending_segment_ids
            || pending.is_empty()
            || pending.len() > 64
            || pending
                .iter()
                .any(|id| id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(ReplicaError::InvalidIdentifier.into());
        }
        let previous = self.snapshot.clone();
        let result = (|| {
            self.merge_replica_gaps_in_memory(gaps)?;
            let checkpoint = self
                .snapshot
                .initial_peer_backfills
                .get_mut(peer_node_id)
                .expect("validated recovery checkpoint");
            checkpoint.summary_pending_segment_ids = pending;
            // The refreshed response may include only part of a new summary page. Re-read that
            // same summary cursor after it drains, so no omitted current segment is skipped.
            checkpoint.summary_pending_next_cursor = checkpoint.summary_cursor.clone();
            checkpoint.summary_complete = false;
            checkpoint.retained_anchor_repair_response_id = None;
            checkpoint.retained_anchor_repair_response_seen = false;
            self.persist_control_state()
        })();
        if result.is_err() {
            self.snapshot = previous;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::load;
    use super::*;

    #[test]
    fn recovery_refresh_keeps_generation_and_cursor_after_restart() {
        let temporary = tempfile::tempdir().unwrap();
        let mut runtime = load(temporary.path());
        let checkpoint = InitialPeerBackfillCheckpoint {
            recovery_generation: 1,
            summary_cursor: Some("same-summary-cursor".to_owned()),
            summary_pending_segment_ids: vec!["1".repeat(64)],
            retained_anchor_repair_response_id: Some("2".repeat(64)),
            ..Default::default()
        };
        runtime
            .snapshot
            .initial_peer_backfills
            .insert("peer".to_owned(), checkpoint.clone());
        runtime
            .refresh_initial_peer_recovery_pending(
                &checkpoint,
                "peer",
                vec!["1".repeat(64), "3".repeat(64)],
                &[],
            )
            .unwrap();
        drop(runtime);
        let mut runtime = load(temporary.path());
        let updated = runtime.initial_peer_backfill_checkpoint("peer").unwrap();
        assert_eq!(updated.recovery_generation, 1);
        assert!(!updated.recovery_generation_consumed);
        assert_eq!(updated.summary_cursor, checkpoint.summary_cursor);
        assert_eq!(updated.summary_pending_next_cursor, updated.summary_cursor);
        assert_eq!(updated.summary_pending_segment_ids.len(), 2);
        assert!(updated.retained_anchor_repair_response_id.is_none());
        assert!(
            runtime
                .refresh_initial_peer_recovery_pending(
                    &checkpoint,
                    "peer",
                    vec!["4".repeat(64)],
                    &[]
                )
                .is_err()
        );
        assert_eq!(
            runtime
                .initial_peer_backfill_checkpoint("peer")
                .unwrap()
                .summary_pending_segment_ids,
            updated.summary_pending_segment_ids
        );
    }
}
