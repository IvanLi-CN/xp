use super::super::ReplicaError;
use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn refresh_initial_peer_recovery_pending(
        &mut self,
        expected: &InitialPeerBackfillCheckpoint,
        peer_node_id: &str,
        pending: Vec<String>,
        gap_pages: [&[RepositoryReplicaGap]; 2],
        refreshed_next_cursor: Option<String>,
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
            || pending.len() > 64
            || pending
                .iter()
                .any(|id| id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(ReplicaError::InvalidIdentifier.into());
        }
        let previous = self.snapshot.clone();
        let result = (|| {
            for gaps in gap_pages {
                self.merge_replica_gaps_in_memory(gaps)?;
            }
            let checkpoint = self
                .snapshot
                .initial_peer_backfills
                .get_mut(peer_node_id)
                .expect("validated recovery checkpoint");
            checkpoint.summary_pending_revisit_cursor = !pending.is_empty();
            checkpoint.summary_pending_segment_ids = pending;
            // The refreshed response may include only part of a new summary page. Re-read that
            // same summary cursor after it drains, so no omitted current segment is skipped.
            if checkpoint.summary_pending_revisit_cursor {
                checkpoint.summary_pending_next_cursor = checkpoint.summary_cursor.clone();
                checkpoint.summary_complete = false;
            } else {
                checkpoint.summary_cursor = refreshed_next_cursor;
                checkpoint.summary_pending_next_cursor = None;
                checkpoint.summary_complete = checkpoint.summary_cursor.is_none();
            }
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
    fn recovery_refresh_accepts_two_full_gap_pages_atomically() {
        let temporary = tempfile::tempdir().unwrap();
        let mut runtime = load(temporary.path());
        let checkpoint = InitialPeerBackfillCheckpoint {
            recovery_generation: 1,
            summary_pending_segment_ids: vec!["1".repeat(64)],
            ..Default::default()
        };
        runtime
            .snapshot
            .initial_peer_backfills
            .insert("peer".to_owned(), checkpoint.clone());
        let gaps = (1..=64)
            .map(|sequence| RepositoryReplicaGap {
                source_node_id: "source".to_owned(),
                source_epoch: 1,
                stream: "connections".to_owned(),
                first_sequence: sequence,
                last_sequence: sequence,
                start_unix_seconds: 1,
                end_unix_seconds: 2,
                permanent: true,
                reason: Some("source_retention_expired".to_owned()),
            })
            .collect::<Vec<_>>();
        runtime
            .refresh_initial_peer_recovery_pending(
                &checkpoint,
                "peer",
                vec!["2".repeat(64)],
                [&gaps, &gaps],
                None,
            )
            .expect("two valid full wire pages must refresh one recovery checkpoint");
        drop(runtime);
        let runtime = load(temporary.path());
        let updated = runtime.initial_peer_backfill_checkpoint("peer").unwrap();
        assert_eq!(updated.recovery_generation, 1);
        assert!(!updated.recovery_generation_consumed);
        assert_eq!(updated.summary_pending_segment_ids, vec!["2".repeat(64)]);
        assert!(updated.summary_pending_revisit_cursor);
        assert_eq!(runtime.snapshot.gaps.len(), 64);
        assert!(runtime.snapshot.gaps.iter().all(|gap| gap.permanent));
    }

    #[test]
    fn recovery_refresh_invalid_second_gap_page_rolls_back_first_page() {
        for oversized in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let mut runtime = load(temporary.path());
            let checkpoint = InitialPeerBackfillCheckpoint {
                recovery_generation: 1,
                summary_pending_segment_ids: vec!["1".repeat(64)],
                ..Default::default()
            };
            runtime
                .snapshot
                .initial_peer_backfills
                .insert("peer".to_owned(), checkpoint.clone());
            runtime.persist_control_state().unwrap();
            let valid = RepositoryReplicaGap {
                source_node_id: "source".to_owned(),
                source_epoch: 1,
                stream: "connections".to_owned(),
                first_sequence: 1,
                last_sequence: 2,
                start_unix_seconds: 1,
                end_unix_seconds: 2,
                permanent: true,
                reason: Some("source_retention_expired".to_owned()),
            };
            let invalid = if oversized {
                vec![valid.clone(); 65]
            } else {
                vec![RepositoryReplicaGap {
                    first_sequence: 3,
                    ..valid.clone()
                }]
            };
            assert!(
                runtime
                    .refresh_initial_peer_recovery_pending(
                        &checkpoint,
                        "peer",
                        vec!["2".repeat(64)],
                        [&[valid], &invalid],
                        None
                    )
                    .is_err()
            );
            assert!(runtime.snapshot.gaps.is_empty());
            assert_eq!(
                runtime
                    .initial_peer_backfill_checkpoint("peer")
                    .unwrap()
                    .summary_pending_segment_ids,
                checkpoint.summary_pending_segment_ids
            );
            drop(runtime);
            let runtime = load(temporary.path());
            assert!(runtime.snapshot.gaps.is_empty());
            let restored = runtime.initial_peer_backfill_checkpoint("peer").unwrap();
            assert_eq!(
                restored.summary_pending_segment_ids,
                checkpoint.summary_pending_segment_ids
            );
            assert_eq!(restored.recovery_generation, 1);
            assert!(!restored.recovery_generation_consumed);
        }
    }

    #[test]
    fn recovery_refresh_empty_page_advances_without_rearming() {
        for next_cursor in [None, Some("next-page".to_owned())] {
            let temporary = tempfile::tempdir().unwrap();
            let mut runtime = load(temporary.path());
            let checkpoint = InitialPeerBackfillCheckpoint {
                recovery_generation: 1,
                summary_pending_segment_ids: vec!["1".repeat(64)],
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
                    Vec::new(),
                    [&[], &[]],
                    next_cursor.clone(),
                )
                .unwrap();
            drop(runtime);
            let runtime = load(temporary.path());
            let updated = runtime.initial_peer_backfill_checkpoint("peer").unwrap();
            assert_eq!(updated.recovery_generation, 1);
            assert!(!updated.recovery_generation_consumed);
            assert!(updated.summary_pending_segment_ids.is_empty());
            assert!(!updated.summary_pending_revisit_cursor);
            assert_eq!(updated.summary_cursor, next_cursor);
            assert_eq!(updated.summary_complete, next_cursor.is_none());
        }
    }

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
                [&[], &[]],
                None,
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
        assert!(updated.summary_pending_revisit_cursor);
        assert!(updated.retained_anchor_repair_response_id.is_none());
        assert!(
            runtime
                .refresh_initial_peer_recovery_pending(
                    &checkpoint,
                    "peer",
                    vec!["4".repeat(64)],
                    [&[], &[]],
                    None,
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
