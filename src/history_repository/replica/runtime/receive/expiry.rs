use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn expire_tombstones(
        &mut self,
        now_unix_seconds: u64,
        mutation: &mut PendingRepositoryMutation,
    ) -> Result<bool, RepositoryRuntimeError> {
        let expired = self.tombstones.expire(now_unix_seconds);
        let removed_tombstones = !expired.is_empty();
        for key in expired {
            if key.known_sequence().is_none()
                && !self.uses_sqlite_history()
                && self
                    .snapshot
                    .records
                    .iter()
                    .any(|record| record.matches_tombstone_identity(&key))
            {
                return Err(RepositoryRuntimeError::Storage(
                    "expired tombstone sequence is ambiguous".into(),
                ));
            }
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
            if !self.tombstones.contains_same_tombstone(&key)
                && let Some(receiver) = self.receiver.as_mut()
            {
                receiver.forget_tombstone(&cursor, &record);
            }
            self.snapshot
                .records
                .retain(|record| !record.matches_key(&key));
            if self.uses_sqlite_history() {
                let (schema_id, schema_version) = key.schema();
                mutation
                    .expired_tombstones
                    .push(RepositoryExpiredTombstone {
                        tombstone: RepositoryHistoryTombstone {
                            source_node_id: key.source_node_id().to_owned(),
                            source_epoch: key.source_epoch(),
                            stream: key.stream().to_owned(),
                            source_stream: None,
                            subject_node_id: key.subject_node_id().to_owned(),
                            observer_node_id: key.observer_node_id().to_owned(),
                            schema_id: schema_id.to_owned(),
                            schema_version,
                            record_key: key.record_key().to_vec(),
                            prefix: false,
                        },
                        sequence: key.known_sequence(),
                    });
                self.snapshot.sequence_summary_blocks_complete = false;
            }
        }
        Ok(removed_tombstones)
    }
}
