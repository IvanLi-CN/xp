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

    pub(crate) fn protect_original_repair_handoff(
        &self,
        identity: &RepositoryNodeIdentity,
        now: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        self.storage
            .refresh_repository_history_export(
                &format!("original-history-import:{}", identity.node_id().as_str()),
                now,
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
    }

    pub(crate) fn original_repair_fingerprint(
        &self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wires: &[(Vec<u8>, u64)],
    ) -> Result<String, RepositoryRuntimeError> {
        let mut digest = Sha256::new();
        digest.update(b"xp-original-history-import-v1\0");
        digest.update(cluster_id.as_bytes());
        digest.update(
            serde_json::to_vec(identity)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?,
        );
        for (wire, _) in wires {
            let segment = SignedSegment::from_wire(wire)?;
            segment.verify_identity(identity)?;
            if segment.canonical().cluster_id() != cluster_id {
                return Err(ProtocolError::ClusterMismatch.into());
            }
            digest.update(Sha256::digest(wire));
        }
        if let Some(receiver) = self.receiver.as_ref() {
            let mut checkpoint = serde_json::to_value(receiver.checkpoint()?)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            // Other live sources must not invalidate an independent historical import.
            if let Some(streams) = checkpoint.get_mut("streams").and_then(|v| v.as_array_mut()) {
                streams.retain(|stream| {
                    stream["key"]["source_node_id"].as_str() == Some(identity.node_id().as_str())
                });
            }
            digest.update(
                serde_json::to_vec(&checkpoint)
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?,
            );
        }
        Ok(hex::encode(digest.finalize()))
    }

    pub(crate) fn receive_repair_wires_from_repository(
        &mut self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wires: &[(Vec<u8>, u64)],
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        self.receive_repair_wires_with_receipts(
            cluster_id,
            identity,
            wires,
            ready_repositories,
            local_repository_id,
        )
        .map(|_| ())
    }

    pub(crate) fn receive_repair_wires_with_receipts(
        &mut self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wires: &[(Vec<u8>, u64)],
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<Vec<RepositorySyncReceipt>, RepositoryRuntimeError> {
        if wires.is_empty() {
            return Ok(Vec::new());
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
        let mut receipts = Vec::with_capacity(wires.len());
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
            match result {
                Ok(receipt) => receipts.push(receipt),
                Err(error) => {
                    self.restore(&previous_receiver, previous_snapshot.clone())?;
                    return Err(error);
                }
            }
        }
        self.persist_or_restore_with_mutation(&previous_receiver, &previous_snapshot, mutation)?;
        // Receipts become authoritative only after the entire original-wire page commits.
        Ok(receipts)
    }
}

#[cfg(test)]
#[path = "repair_receipt_tests.rs"]
mod repair_receipt_tests;
