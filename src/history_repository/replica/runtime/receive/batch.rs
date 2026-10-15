use super::*;

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
            let result = self.receive_wire_from_repository_inner(
                RepositoryWireRequest {
                    cluster_id,
                    identity: &segment.identity,
                    wire: &segment.wire,
                    now_unix_seconds,
                    ready_repositories,
                    local_repository_id,
                    allow_retained_anchor: false,
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
