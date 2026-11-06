use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn receive_wire(
        &mut self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wire: &[u8],
        now_unix_seconds: u64,
    ) -> Result<RepositorySyncReceipt, RepositoryRuntimeError> {
        self.receive_wire_from_repository(
            cluster_id,
            identity,
            wire,
            now_unix_seconds,
            &["local".to_owned()],
            "local",
        )
    }

    pub(crate) fn receive_wire_from_repository(
        &mut self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wire: &[u8],
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<RepositorySyncReceipt, RepositoryRuntimeError> {
        self.receive_wire_from_repository_inner(
            RepositoryWireRequest {
                cluster_id,
                identity,
                wire,
                now_unix_seconds,
                ready_repositories,
                local_repository_id,
                allow_retained_anchor: false,
                allow_verified_local_source_replay: false,
                rollback_snapshot: None,
                discard_gap_evidence_on_error: false,
                allow_retained_sequence_gap: false,
                retained_anchor_checkpoint: None,
            },
            None,
        )
    }

    pub(crate) fn receive_repair_wires_from_repository(
        &mut self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wires: &[(Vec<u8>, u64)],
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        if wires.is_empty() {
            return Ok(());
        }
        self.rebuild_if_stale(wires[0].1)?;
        self.bind_cluster(cluster_id)?;
        self.ensure_receiver()?;
        let previous_receiver = self
            .receiver
            .as_ref()
            .expect("receiver initialized")
            .checkpoint()?;
        let previous_snapshot = self.snapshot.clone();
        let mut mutation = PendingRepositoryMutation {
            target_repository_id: local_repository_id.to_owned(),
            progress_at_unix_seconds: wires.iter().map(|(_, at)| *at).max(),
            ..PendingRepositoryMutation::default()
        };
        for (wire, now_unix_seconds) in wires {
            let result = self.receive_wire_from_repository_inner_without_rebuild(
                RepositoryWireRequest {
                    cluster_id,
                    identity,
                    wire,
                    now_unix_seconds: *now_unix_seconds,
                    ready_repositories,
                    local_repository_id,
                    allow_retained_anchor: false,
                    allow_verified_local_source_replay: false,
                    rollback_snapshot: None,
                    discard_gap_evidence_on_error: true,
                    allow_retained_sequence_gap: false,
                    retained_anchor_checkpoint: None,
                },
                Some(&mut mutation),
                false,
                true,
            );
            if let Err(error) = result {
                self.restore(&previous_receiver, previous_snapshot.clone())?;
                return Err(error);
            }
        }
        self.persist_or_restore_with_mutation(&previous_receiver, &previous_snapshot, mutation)
    }
}
