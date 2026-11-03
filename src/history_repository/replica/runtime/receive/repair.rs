use super::*;

impl RepositoryReplicaRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn receive_repair_wire_from_repository_with_gaps(
        &mut self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wire: &[u8],
        gaps: &[RepositoryReplicaGap],
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<RepositorySyncReceipt, RepositoryRuntimeError> {
        self.rebuild_if_stale(now_unix_seconds)?;
        let previous_snapshot = self.snapshot.clone();
        let previous_tombstones = self.tombstones.checkpoint();
        if let Err(error) = self.merge_replica_gaps_in_memory(gaps) {
            self.snapshot = previous_snapshot;
            self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones.clone())?;
            return Err(error);
        }
        let result = self.receive_wire_from_repository_inner_without_rebuild(
            RepositoryWireRequest {
                cluster_id,
                identity,
                wire,
                now_unix_seconds,
                ready_repositories,
                local_repository_id,
                allow_retained_anchor: false,
                rollback_snapshot: Some(previous_snapshot.clone()),
                discard_gap_evidence_on_error: true,
                allow_retained_sequence_gap: false,
                retained_anchor_checkpoint: None,
            },
            None,
            true,
            true,
        );
        match result {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                if !matches!(
                    &error,
                    RepositoryRuntimeError::Protocol(
                        ProtocolError::SequenceGap { .. }
                            | ProtocolError::EpochGap { .. }
                            | ProtocolError::ForkDetected { .. }
                    )
                ) {
                    self.snapshot = previous_snapshot;
                    self.tombstones =
                        TombstoneLedger::from_checkpoint(previous_tombstones.clone())?;
                }
                Err(error)
            }
        }
    }
}

// A historical record may be suppressed only by an already applied tombstone or by a
// matching tombstone in this same signed segment, committed with the receipt.
pub(super) fn is_known_deleted(
    tombstones: &TombstoneLedger,
    segment: &CanonicalSegment,
    cursor: &Cursor,
    record: &SyncRecord,
) -> bool {
    if !is_known_schema(record) || record.is_tombstone() {
        return false;
    }
    let make_key = |position: &Cursor, item: &SyncRecord| {
        let (schema, version) = item.schema();
        ReplicaCursor::new(
            position.source_node_id(),
            position.source_epoch(),
            position.stream(),
            position.sequence(),
        )
        .and_then(|replica_cursor| {
            ReplicaRecord::new(
                &replica_cursor,
                item.subject_node_id(),
                item.observer_node_id(),
                schema,
                version,
                item.record_key().to_vec(),
                item.payload_bytes().to_vec(),
            )
        })
        .map(|replica_record| replica_record.key())
    };
    let Ok(key) = make_key(cursor, record) else {
        return false;
    };
    if !tombstones.allows(&key) {
        return true;
    }
    segment.records().iter().enumerate().any(|(offset, item)| {
        if !item.is_tombstone() || !is_known_schema(item) {
            return false;
        }
        let Some(sequence) = segment.first_cursor().sequence().checked_add(offset as u64) else {
            return false;
        };
        let Ok(position) = Cursor::new(
            cursor.source_node_id(),
            cursor.source_epoch(),
            cursor.stream(),
            sequence,
        ) else {
            return false;
        };
        make_key(&position, item)
            .is_ok_and(|tombstone| TombstoneLedger::tombstone_blocks(&tombstone, &key))
    })
}

impl RepositoryReplicaRuntime {
    // Legacy signed streams may contain ordinary records beside tombstones. Preserve the
    // tombstone's original stream as an additional, source-scoped deletion target.
    pub(crate) fn protect_tombstone_origin(
        &mut self,
        key: &ReplicaRecordKey,
        mutation: &mut PendingRepositoryMutation,
    ) -> Result<(), RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            let target = mutation.tombstones.last_mut().ok_or_else(|| {
                RepositoryRuntimeError::Storage("missing tombstone mutation".to_owned())
            })?;
            target.source_stream = Some(key.stream().to_owned());
        } else {
            self.snapshot.records.retain(|record| {
                record.tombstone
                    || record.source_node_id != key.source_node_id()
                    || record.source_epoch != key.source_epoch()
                    || record.stream != key.stream()
                    || !record.matches_tombstone_key(key, key.record_key().ends_with(b":"))
            });
        }
        Ok(())
    }
}
