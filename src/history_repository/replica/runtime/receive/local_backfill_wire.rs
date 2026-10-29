use super::*;

impl RepositoryReplicaRuntime {
    pub(super) fn receive_wire_from_repository_inner(
        &mut self,
        request: RepositoryWireRequest<'_>,
        mutation_sink: Option<&mut PendingRepositoryMutation>,
    ) -> Result<RepositorySyncReceipt, RepositoryRuntimeError> {
        self.receive_wire_from_repository_inner_without_rebuild(request, mutation_sink, true)
    }

    pub(crate) fn receive_local_history_backfill_wire(
        &mut self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wire: &[u8],
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<RepositorySyncReceipt, RepositoryRuntimeError> {
        // Skipping destructive rebuild must not let expired pending wire establish freshness.
        self.ensure_local_history_backfill_is_fresh(now_unix_seconds)?;
        self.receive_wire_from_repository_inner_without_rebuild(
            RepositoryWireRequest {
                cluster_id,
                identity,
                wire,
                now_unix_seconds,
                ready_repositories,
                local_repository_id,
                allow_retained_anchor: false,
                rollback_snapshot: None,
                discard_gap_evidence_on_error: false,
                allow_retained_sequence_gap: false,
                retained_anchor_checkpoint: None,
            },
            None,
            false,
        )
    }

    pub(crate) fn receive_local_history_backfill_legacy_page(
        &mut self,
        cluster_id: &str,
        segments: &[RepositoryReplicaSegment],
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<Vec<RepositorySyncReceipt>, RepositoryRuntimeError> {
        self.receive_wire_batch_from_repository_with_gaps(
            cluster_id,
            segments,
            &[],
            now_unix_seconds,
            ready_repositories,
            local_repository_id,
        )
    }
}
