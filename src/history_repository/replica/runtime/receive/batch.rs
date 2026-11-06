use super::*;
use std::collections::BTreeSet;

impl RepositoryReplicaRuntime {
    pub(crate) fn receive_wire_batch_from_repository_with_gaps(
        &mut self,
        cluster_id: &str,
        segments: &[RepositoryReplicaSegment],
        gaps: &[RepositoryReplicaGap],
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<Vec<RepositorySyncReceipt>, RepositoryRuntimeError> {
        self.receive_wire_batch_from_repository_with_gaps_mode(
            cluster_id,
            segments,
            gaps,
            now_unix_seconds,
            ready_repositories,
            local_repository_id,
            false,
            None,
        )
    }

    pub(crate) fn verified_local_source_replay_streams(
        &mut self,
        cluster_id: &str,
        segments: &[RepositoryReplicaSegment],
        now_unix_seconds: u64,
    ) -> Result<BTreeSet<String>, RepositoryRuntimeError> {
        if segments.is_empty() {
            return Ok(BTreeSet::new());
        }
        self.rebuild_if_stale(now_unix_seconds)?;
        self.bind_cluster(cluster_id)?;
        self.ensure_receiver()?;
        let signed_segments = segments
            .iter()
            .map(|segment| SignedSegment::from_wire(&segment.wire));
        Ok(self
            .receiver
            .as_ref()
            .expect("receiver initialized")
            .verified_local_source_replay_streams(signed_segments)?)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn receive_local_source_batch_from_repository_with_gaps(
        &mut self,
        cluster_id: &str,
        segments: &[RepositoryReplicaSegment],
        gaps: &[RepositoryReplicaGap],
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
        verified_replay_streams: &BTreeSet<String>,
    ) -> Result<Vec<RepositorySyncReceipt>, RepositoryRuntimeError> {
        self.receive_wire_batch_from_repository_with_gaps_mode(
            cluster_id,
            segments,
            gaps,
            now_unix_seconds,
            ready_repositories,
            local_repository_id,
            true,
            Some(verified_replay_streams),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn receive_wire_batch_from_repository_with_gaps_mode(
        &mut self,
        cluster_id: &str,
        segments: &[RepositoryReplicaSegment],
        gaps: &[RepositoryReplicaGap],
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
        allow_retained_anchor: bool,
        verified_replay_streams: Option<&BTreeSet<String>>,
    ) -> Result<Vec<RepositorySyncReceipt>, RepositoryRuntimeError> {
        if segments.is_empty() {
            return Ok(Vec::new());
        }
        self.rebuild_if_stale(now_unix_seconds)?;
        self.bind_cluster(cluster_id)?;
        self.ensure_receiver()?;
        let previous_snapshot = self.snapshot.clone();
        let previous_tombstones = self.tombstones.checkpoint();
        let previous_receiver = self
            .receiver
            .as_ref()
            .expect("receiver initialized after rebuild")
            .checkpoint()?;
        if let Err(error) = self.merge_replica_gaps_in_memory(gaps) {
            self.snapshot = previous_snapshot;
            self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
            return Err(error);
        }
        let mut mutation = PendingRepositoryMutation {
            target_repository_id: local_repository_id.to_owned(),
            progress_at_unix_seconds: Some(now_unix_seconds),
            ..PendingRepositoryMutation::default()
        };
        let mut receipts = Vec::with_capacity(segments.len());
        for segment in segments {
            let allow_verified_local_source_replay =
                verified_replay_streams.is_some_and(|streams| {
                    SignedSegment::from_wire(&segment.wire).is_ok_and(|signed| {
                        let cursor = signed.canonical().first_cursor();
                        streams.contains(&format!(
                            "{}:{}:{}",
                            cursor.source_node_id(),
                            cursor.source_epoch(),
                            cursor.stream()
                        ))
                    })
                });
            let result = self.receive_wire_from_repository_inner(
                RepositoryWireRequest {
                    cluster_id,
                    identity: &segment.identity,
                    wire: &segment.wire,
                    now_unix_seconds,
                    ready_repositories,
                    local_repository_id,
                    allow_retained_anchor,
                    allow_verified_local_source_replay,
                    rollback_snapshot: Some(previous_snapshot.clone()),
                    discard_gap_evidence_on_error: true,
                    allow_retained_sequence_gap: false,
                    retained_anchor_checkpoint: None,
                },
                Some(&mut mutation),
            );
            match result {
                Ok(receipt) => receipts.push(receipt),
                Err(error) => {
                    self.restore(&previous_receiver, previous_snapshot.clone())?;
                    self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
                    // Batch application is atomic. The caller must retry the same durable
                    // segment set through the normal single-page path when a batch fails;
                    // writing a fallback segment here would report failure after a partial
                    // commit and could duplicate the pending ACK on replay.
                    return Err(error);
                }
            }
        }
        self.persist_or_restore_with_mutation(&previous_receiver, &previous_snapshot, mutation)?;
        if let Err(error) = self.prune_retention(now_unix_seconds) {
            self.storage_degraded = true;
            tracing::warn!(
                error = %error,
                "history retention maintenance deferred after batch receive"
            );
        }
        Ok(receipts)
    }
}
