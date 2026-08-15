use super::*;

#[derive(Default)]
pub(crate) struct PendingRepositoryMutation {
    pub(crate) records: Vec<RepositoryHistoryRecordRow>,
    pub(crate) tombstones: Vec<RepositoryHistoryTombstone>,
    pub(crate) segments: Vec<RepositoryHistorySegmentRow>,
}

impl PendingRepositoryMutation {
    pub(super) fn into_storage_mutation(self) -> RepositoryReplicaMutation {
        RepositoryReplicaMutation {
            records: self.records,
            tombstones: self.tombstones,
            segments: self.segments,
        }
    }
}

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
        self.rebuild_if_stale(now_unix_seconds)?;
        self.refresh_capacity()?;
        let availability = self.snapshot.capacity.history_write_availability();
        if !availability.allows_history_writes() {
            return Err(RepositoryRuntimeError::WriteStopped(availability));
        }
        let segment = SignedSegment::from_wire(wire)?;
        self.bind_cluster(cluster_id)?;
        self.ensure_receiver()?;
        let previous_receiver = self
            .receiver
            .as_ref()
            .expect("receiver initialized")
            .checkpoint()?;
        let previous_snapshot = self.snapshot.clone();
        self.tombstones
            .reconcile_ready_repositories(ready_repositories)?;
        let expired_tombstones = self.expire_tombstones(now_unix_seconds)?;
        let acceptance = self
            .receiver
            .as_mut()
            .expect("receiver initialized")
            .accept(&segment, identity);
        let acceptance = match acceptance {
            Ok(acceptance) => acceptance,
            Err(error) => {
                let records_gap = matches!(
                    error,
                    ProtocolError::SequenceGap { .. }
                        | ProtocolError::EpochGap { .. }
                        | ProtocolError::ForkDetected { .. }
                );
                if let ProtocolError::SequenceGap { expected, actual } = error {
                    self.record_sequence_gap(segment.canonical(), expected, actual);
                } else if records_gap {
                    self.record_gap(segment.canonical(), true);
                }
                if records_gap || expired_tombstones {
                    self.persist_or_restore(&previous_receiver, &previous_snapshot)?;
                }
                return Err(error.into());
            }
        };
        if matches!(acceptance, Acceptance::Duplicate { .. }) {
            self.persist_or_restore(&previous_receiver, &previous_snapshot)?;
            return Ok(sync_receipt(acceptance, availability, Vec::new()));
        }

        if let Some(gap) = acceptance.gap() {
            self.record_epoch_rotation_gap(gap, segment.canonical().opened_at_unix_seconds());
        }
        let mut mutation = PendingRepositoryMutation::default();
        let tombstone_acknowledgements = match self.append_known_records(
            segment.canonical(),
            now_unix_seconds,
            ready_repositories,
            local_repository_id,
            &mut mutation,
        ) {
            Ok(acknowledgements) => acknowledgements,
            Err(error) => {
                self.restore(&previous_receiver, previous_snapshot)?;
                return Err(error);
            }
        };
        if let Err(error) = self.store_segment(identity, wire, &mut mutation) {
            self.restore(&previous_receiver, previous_snapshot)?;
            return Err(error);
        }
        self.clear_repaired_gaps(segment.canonical());
        self.record_source_received(segment.canonical(), now_unix_seconds)?;
        // A forward keyset cursor must survive continuous ingestion. New rows at or before the
        // completed cursor are handled by a bounded restart below; newer rows are reached by the
        // current pass without starving the rest of the retained window.
        self.reopen_retention_compaction_for_late_record(segment.canonical());
        self.snapshot.last_verified_unix_seconds = Some(now_unix_seconds);
        self.persist_or_restore_with_mutation(&previous_receiver, &previous_snapshot, mutation)?;
        // Retention is a separate, retryable maintenance pass. It must never make a just-accepted
        // segment half durable with its control checkpoint.
        self.prune_retention(now_unix_seconds)?;
        Ok(sync_receipt(
            acceptance,
            availability,
            tombstone_acknowledgements,
        ))
    }

    pub(super) fn bind_cluster(&mut self, cluster_id: &str) -> Result<(), RepositoryRuntimeError> {
        match self.snapshot.cluster_id.as_deref() {
            Some(existing) if existing != cluster_id => {
                Err(RepositoryRuntimeError::ClusterBindingMismatch)
            }
            Some(_) => Ok(()),
            None => {
                self.snapshot.cluster_id = Some(cluster_id.to_owned());
                Ok(())
            }
        }
    }

    fn ensure_receiver(&mut self) -> Result<(), RepositoryRuntimeError> {
        if self.receiver.is_none() {
            let cluster_id = self
                .snapshot
                .cluster_id
                .clone()
                .ok_or(RepositoryRuntimeError::ClusterBindingMismatch)?;
            self.receiver = Some(SegmentReceiver::for_cluster(cluster_id, known_schemas()));
        }
        Ok(())
    }

    pub(super) fn append_known_records(
        &mut self,
        segment: &crate::history_sync::CanonicalSegment,
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
        mutation: &mut PendingRepositoryMutation,
    ) -> Result<Vec<RepositoryTombstoneAcknowledgement>, RepositoryRuntimeError> {
        let mut acknowledgements = Vec::new();
        for (offset, record) in segment.records().iter().enumerate() {
            if !is_known_schema(record) {
                continue;
            }
            let sequence = segment
                .first_cursor()
                .sequence()
                .checked_add(
                    u64::try_from(offset)
                        .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
                )
                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
            let (schema_id, schema_version) = record.schema();
            let cursor = ReplicaCursor::new(
                segment.first_cursor().source_node_id(),
                segment.first_cursor().source_epoch(),
                segment.first_cursor().stream(),
                sequence,
            )?;
            let replica_record = ReplicaRecord::new(
                &cursor,
                record.subject_node_id(),
                record.observer_node_id(),
                schema_id,
                schema_version,
                record.record_key().to_vec(),
                record.payload_bytes().to_vec(),
            )?;
            let key = replica_record.key();
            if record.is_tombstone() {
                let target_stream = source_stream_for_schema(schema_id).ok_or_else(|| {
                    RepositoryRuntimeError::Storage(format!(
                        "tombstone schema has no target stream: {schema_id}"
                    ))
                })?;
                let target_cursor = ReplicaCursor::new(
                    segment.first_cursor().source_node_id(),
                    segment.first_cursor().source_epoch(),
                    target_stream,
                    sequence,
                )?;
                let target_key = ReplicaRecord::new(
                    &target_cursor,
                    record.subject_node_id(),
                    record.observer_node_id(),
                    schema_id,
                    schema_version,
                    record.record_key().to_vec(),
                    record.payload_bytes().to_vec(),
                )?
                .key();
                self.delete_records_for_tombstone(&target_key, mutation)?;
                self.tombstones
                    .tombstone(key, now_unix_seconds, ready_repositories)?;
                self.tombstones
                    .acknowledge(replica_record.key(), local_repository_id)?;
                acknowledgements.push(RepositoryTombstoneAcknowledgement {
                    key: replica_record.key(),
                    repository_id: local_repository_id.to_owned(),
                });
                // Query paths exclude this row, while the retained source cursor allows a joining
                // repository to reconstruct deletion protection after the signed repair cache
                // has expired.
                let stored = StoredRecord::from_record(
                    segment.closed_at_unix_seconds(),
                    now_unix_seconds,
                    &cursor,
                    record,
                );
                if self.uses_sqlite_history() {
                    mutation.records.push(stored.sqlite_row()?);
                } else {
                    self.snapshot.records.push(stored);
                }
                continue;
            } else if !self.tombstones.allows(&key) {
                return Err(RepositoryRuntimeError::Protocol(
                    ProtocolError::ResurrectionPrevented,
                ));
            }
            let stored = StoredRecord::from_record(
                segment.closed_at_unix_seconds(),
                now_unix_seconds,
                &cursor,
                record,
            );
            if self.uses_sqlite_history() {
                mutation.records.push(stored.sqlite_row()?);
            } else {
                self.snapshot.records.push(stored);
            }
        }
        Ok(acknowledgements)
    }

    pub(super) fn clear_repaired_gaps(&mut self, segment: &crate::history_sync::CanonicalSegment) {
        self.snapshot.gaps.retain(|gap| {
            gap.permanent
                || gap.source_node_id != segment.first_cursor().source_node_id()
                || gap.source_epoch != segment.first_cursor().source_epoch()
                || gap.stream != segment.first_cursor().stream()
                || segment.first_cursor().sequence() > gap.first_sequence
                || segment.last_cursor().sequence() < gap.last_sequence
        });
    }

    pub(super) fn reopen_retention_compaction_for_late_record(
        &mut self,
        segment: &crate::history_sync::CanonicalSegment,
    ) {
        let Some(cursor) = self.snapshot.retention_compaction_cursor.as_ref() else {
            return;
        };
        let first = segment.first_cursor();
        let incoming = (
            segment.opened_at_unix_seconds(),
            first.source_node_id(),
            first.source_epoch(),
            first.stream(),
            first.sequence(),
        );
        let completed = (
            cursor.observed_start_unix_seconds,
            cursor.source_node_id.as_str(),
            cursor.source_epoch,
            cursor.stream.as_str(),
            cursor.sequence,
        );
        if incoming <= completed {
            self.snapshot.retention_compaction_cursor = None;
            self.snapshot.retention_compaction_continuation = None;
        }
    }

    pub(super) fn delete_records_for_tombstone(
        &mut self,
        key: &ReplicaRecordKey,
        mutation: &mut PendingRepositoryMutation,
    ) -> Result<(), RepositoryRuntimeError> {
        let (schema_id, schema_version) = key.schema();
        let prefix = key.record_key().ends_with(b":");
        if self.uses_sqlite_history() {
            mutation.tombstones.push(RepositoryHistoryTombstone {
                source_node_id: key.source_node_id().to_owned(),
                source_epoch: key.source_epoch(),
                stream: key.stream().to_owned(),
                subject_node_id: key.subject_node_id().to_owned(),
                observer_node_id: key.observer_node_id().to_owned(),
                schema_id: schema_id.to_owned(),
                schema_version,
                record_key: key.record_key().to_vec(),
                prefix,
            });
        } else {
            self.snapshot
                .records
                .retain(|record| record.tombstone || !record.matches_tombstone_key(key, prefix));
        }
        Ok(())
    }

    pub(super) fn persist_or_restore(
        &mut self,
        previous_receiver: &SegmentReceiverCheckpoint,
        previous_snapshot: &RepositoryReplicaSnapshot,
    ) -> Result<(), RepositoryRuntimeError> {
        self.persist_or_restore_with_mutation(
            previous_receiver,
            previous_snapshot,
            PendingRepositoryMutation::default(),
        )
    }

    pub(super) fn persist_or_restore_with_mutation(
        &mut self,
        previous_receiver: &SegmentReceiverCheckpoint,
        previous_snapshot: &RepositoryReplicaSnapshot,
        mutation: PendingRepositoryMutation,
    ) -> Result<(), RepositoryRuntimeError> {
        self.snapshot.receiver = Some(
            self.receiver
                .as_ref()
                .expect("receiver initialized")
                .checkpoint()?,
        );
        self.snapshot.tombstones = self.tombstones.checkpoint();
        let bytes = serde_json::to_vec(&self.snapshot)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if bytes.len() > MAX_RUNTIME_STATE_BYTES {
            self.restore(previous_receiver, previous_snapshot.clone())?;
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let result = if self.uses_sqlite_history() {
            self.storage
                .commit_repository_replica_mutation(mutation.into_storage_mutation(), &bytes)
        } else {
            self.storage.write(REPOSITORY_REPLICA_KEY, &bytes)
        };
        if let Err(error) = result {
            self.restore(previous_receiver, previous_snapshot.clone())?;
            return Err(RepositoryRuntimeError::Storage(error.to_string()));
        }
        Ok(())
    }

    pub(super) fn persist_import_mutation(
        &mut self,
        previous_snapshot: RepositoryReplicaSnapshot,
        mutation: PendingRepositoryMutation,
    ) -> Result<(), RepositoryRuntimeError> {
        self.snapshot.tombstones = self.tombstones.checkpoint();
        let bytes = serde_json::to_vec(&self.snapshot)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if bytes.len() > MAX_RUNTIME_STATE_BYTES {
            self.snapshot = previous_snapshot;
            self.tombstones = TombstoneLedger::from_checkpoint(self.snapshot.tombstones.clone())?;
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let result = if self.uses_sqlite_history() {
            self.storage
                .commit_repository_replica_mutation(mutation.into_storage_mutation(), &bytes)
        } else {
            self.storage.write(REPOSITORY_REPLICA_KEY, &bytes)
        };
        if let Err(error) = result {
            self.snapshot = previous_snapshot;
            self.tombstones = TombstoneLedger::from_checkpoint(self.snapshot.tombstones.clone())?;
            return Err(RepositoryRuntimeError::Storage(error.to_string()));
        }
        Ok(())
    }

    pub(super) fn restore(
        &mut self,
        previous_receiver: &SegmentReceiverCheckpoint,
        previous_snapshot: RepositoryReplicaSnapshot,
    ) -> Result<(), RepositoryRuntimeError> {
        let cluster_id = self
            .snapshot
            .cluster_id
            .clone()
            .ok_or(RepositoryRuntimeError::ClusterBindingMismatch)?;
        self.receiver = Some(SegmentReceiver::from_checkpoint(
            cluster_id,
            known_schemas(),
            previous_receiver.clone(),
        )?);
        self.snapshot = previous_snapshot;
        self.tombstones = TombstoneLedger::from_checkpoint(self.snapshot.tombstones.clone())?;
        Ok(())
    }

    pub(super) fn record_source_received_from_cursor(
        &mut self,
        cursor: &ReplicaCursor,
        now_unix_seconds: u64,
    ) {
        let source_node_id = cursor.source_node_id().to_owned();
        if !self
            .snapshot
            .source_last_received_unix_seconds
            .contains_key(&source_node_id)
            && self.snapshot.source_last_received_unix_seconds.len() == 4_096
        {
            self.snapshot.history_truncated = true;
            return;
        }
        self.snapshot
            .source_last_received_unix_seconds
            .insert(source_node_id, now_unix_seconds);
    }

    pub(super) fn rebuild_if_stale(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        let Some(last_verified) = self.snapshot.last_verified_unix_seconds else {
            return Ok(());
        };
        if !ReplicaFreshness::new(last_verified, TOMBSTONE_HORIZON_SECONDS)
            .requires_rebuild(now_unix_seconds)
        {
            return Ok(());
        }
        let cluster_id = self.snapshot.cluster_id.clone();
        let mut local_source = self.snapshot.local_source.clone();
        local_source.rotate_after_repository_rebuild();
        if let (Some(cluster_id), Some(node_id)) = (cluster_id.as_deref(), local_source.node_id()) {
            self.storage
                .record_repository_source_epoch(cluster_id, node_id, local_source.epoch())
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        }
        if self.storage.is_sqlite() {
            self.storage
                .clear_repository_history()
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        }
        self.snapshot = RepositoryReplicaSnapshot {
            cluster_id: cluster_id.clone(),
            external_history: self.storage.is_sqlite(),
            local_source,
            ..RepositoryReplicaSnapshot::default()
        };
        self.tombstones = TombstoneLedger::new(TOMBSTONE_HORIZON_SECONDS);
        self.receiver =
            cluster_id.map(|cluster_id| SegmentReceiver::for_cluster(cluster_id, known_schemas()));
        self.persist_control_state()
    }

    pub(super) fn expire_tombstones(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<bool, RepositoryRuntimeError> {
        let expired = self.tombstones.expire(now_unix_seconds);
        let removed_tombstones = !expired.is_empty();
        for key in expired {
            let cursor = Cursor::new(
                key.source_node_id(),
                key.source_epoch(),
                key.stream(),
                key.sequence(),
            )?;
            let (schema_id, schema_version) = key.schema();
            let record = SyncRecord::new(
                key.subject_node_id(),
                key.observer_node_id(),
                schema_id,
                schema_version,
                key.record_key().to_vec(),
                Vec::new(),
                true,
            );
            if let Some(receiver) = self.receiver.as_mut() {
                receiver.forget_tombstone(&cursor, &record);
            }
            self.snapshot
                .records
                .retain(|record| !record.matches_key(&key));
            if self.uses_sqlite_history() {
                let (schema_id, schema_version) = key.schema();
                self.storage
                    .delete_repository_history_tombstone(&RepositoryHistoryTombstone {
                        source_node_id: key.source_node_id().to_owned(),
                        source_epoch: key.source_epoch(),
                        stream: key.stream().to_owned(),
                        subject_node_id: key.subject_node_id().to_owned(),
                        observer_node_id: key.observer_node_id().to_owned(),
                        schema_id: schema_id.to_owned(),
                        schema_version,
                        record_key: key.record_key().to_vec(),
                        prefix: false,
                    })
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            }
        }
        Ok(removed_tombstones)
    }
}

pub(crate) fn source_stream_for_schema(schema_id: &str) -> Option<&'static str> {
    Some(match schema_id {
        "runtime.v1" => "runtime",
        "path_health.v1" => "path_health",
        "traffic.v1" => "traffic",
        "connections.v1" => "connections",
        "ip_usage.v1" => "ip_usage",
        _ => return None,
    })
}
