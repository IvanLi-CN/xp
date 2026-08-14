use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Clone)]
pub(crate) struct RepositoryTieredBackfillRecord {
    pub(crate) observed_at_unix_seconds: u64,
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
    pub(crate) sequence: u64,
    pub(crate) subject_node_id: String,
    pub(crate) observer_node_id: String,
    pub(crate) schema_id: String,
    pub(crate) schema_version: u32,
    pub(crate) record_key: Vec<u8>,
    pub(crate) payload: Vec<u8>,
    pub(crate) tombstone: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryTieredBackfillPage {
    pub(crate) records: Vec<RepositoryTieredBackfillRecord>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepositoryTieredBackfillCursor {
    repair_cache_cutoff_unix_seconds: u64,
    received_at_cutoff_unix_seconds: u64,
    tombstone_high_watermark: Option<RepositoryHistoryCompactionCursor>,
    record_high_watermark: Option<RepositoryHistoryCompactionCursor>,
    #[serde(default)]
    phase: RepositoryTieredBackfillPhase,
    export_session_id: String,
    #[serde(default)]
    after: Option<RepositoryHistoryCompactionCursor>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RepositoryTieredBackfillPhase {
    #[default]
    Tombstones,
    Records,
}

enum RepositoryTieredBackfillCursorState {
    Current(RepositoryTieredBackfillCursor),
    Legacy(RepositoryHistoryCompactionCursor),
}

impl RepositoryReplicaRuntime {
    pub(crate) fn tiered_backfill_page(
        &self,
        page_cursor: Option<&str>,
        limit: usize,
        repair_cache_cutoff_unix_seconds: u64,
        now_unix_seconds: u64,
    ) -> Result<RepositoryTieredBackfillPage, RepositoryRuntimeError> {
        let cursor = page_cursor
            .map(|cursor| {
                if cursor.len() > 1_024 {
                    return Err(RepositoryRuntimeError::StateLimitExceeded);
                }
                let bytes = URL_SAFE_NO_PAD
                    .decode(cursor)
                    .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?;
                serde_json::from_slice::<RepositoryTieredBackfillCursor>(&bytes)
                    .map(RepositoryTieredBackfillCursorState::Current)
                    .or_else(|_| {
                        serde_json::from_slice::<RepositoryHistoryCompactionCursor>(&bytes)
                            .map(RepositoryTieredBackfillCursorState::Legacy)
                    })
                    .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)
            })
            .transpose()?;
        let (
            after,
            repair_cache_cutoff_unix_seconds,
            received_at_cutoff_unix_seconds,
            tombstone_high_watermark,
            record_high_watermark,
            phase,
            export_session_id,
        ) = match cursor {
            Some(RepositoryTieredBackfillCursorState::Current(cursor)) => {
                if !self
                    .storage
                    .has_repository_history_export_session(
                        &cursor.export_session_id,
                        now_unix_seconds,
                    )
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
                {
                    return Err(RepositoryRuntimeError::StateLimitExceeded);
                }
                (
                    cursor.after,
                    cursor.repair_cache_cutoff_unix_seconds,
                    cursor.received_at_cutoff_unix_seconds,
                    cursor.tombstone_high_watermark,
                    cursor.record_high_watermark,
                    cursor.phase,
                    cursor.export_session_id,
                )
            }
            Some(RepositoryTieredBackfillCursorState::Legacy(after)) => {
                let Some((
                    tombstone_high_watermark,
                    record_high_watermark,
                    received_at_cutoff_unix_seconds,
                )) = self
                    .storage
                    .repository_history_export_watermarks(repair_cache_cutoff_unix_seconds)
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
                else {
                    return Ok(RepositoryTieredBackfillPage {
                        records: Vec::new(),
                        next_cursor: None,
                    });
                };
                (
                    Some(after),
                    repair_cache_cutoff_unix_seconds,
                    received_at_cutoff_unix_seconds,
                    tombstone_high_watermark,
                    record_high_watermark,
                    RepositoryTieredBackfillPhase::Records,
                    uuid::Uuid::new_v4().to_string(),
                )
            }
            None => {
                let Some((
                    tombstone_high_watermark,
                    record_high_watermark,
                    received_at_cutoff_unix_seconds,
                )) = self
                    .storage
                    .repository_history_export_watermarks(repair_cache_cutoff_unix_seconds)
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
                else {
                    return Ok(RepositoryTieredBackfillPage {
                        records: Vec::new(),
                        next_cursor: None,
                    });
                };
                let phase = if tombstone_high_watermark.is_some() {
                    RepositoryTieredBackfillPhase::Tombstones
                } else {
                    RepositoryTieredBackfillPhase::Records
                };
                (
                    None,
                    repair_cache_cutoff_unix_seconds,
                    received_at_cutoff_unix_seconds,
                    tombstone_high_watermark,
                    record_high_watermark,
                    phase,
                    uuid::Uuid::new_v4().to_string(),
                )
            }
        };
        let high_watermark = match phase {
            RepositoryTieredBackfillPhase::Tombstones => tombstone_high_watermark.as_ref(),
            RepositoryTieredBackfillPhase::Records => record_high_watermark.as_ref(),
        };
        let Some(high_watermark) = high_watermark else {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        };
        self.storage
            .refresh_repository_history_export(&export_session_id, now_unix_seconds)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let mut rows = self
            .storage
            .repository_history_records_page(
                after.as_ref(),
                high_watermark,
                limit.saturating_add(1),
                matches!(phase, RepositoryTieredBackfillPhase::Tombstones),
                repair_cache_cutoff_unix_seconds,
                received_at_cutoff_unix_seconds,
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next_phase = if has_more {
            Some(phase)
        } else if matches!(phase, RepositoryTieredBackfillPhase::Tombstones)
            && record_high_watermark.is_some()
        {
            Some(RepositoryTieredBackfillPhase::Records)
        } else {
            None
        };
        if next_phase.is_none() {
            self.storage
                .finish_repository_history_export(&export_session_id)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        }
        let next_cursor = next_phase.map(|next_phase| {
            let next_after = if has_more {
                rows.last().map(RepositoryHistoryCompactionCursor::from)
            } else {
                None
            };
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&RepositoryTieredBackfillCursor {
                    repair_cache_cutoff_unix_seconds,
                    received_at_cutoff_unix_seconds,
                    tombstone_high_watermark,
                    record_high_watermark,
                    phase: next_phase,
                    export_session_id: export_session_id.clone(),
                    after: next_after,
                })
                .expect("repository backfill cursor is serializable"),
            )
        });
        let records = rows
            .into_iter()
            .map(StoredRecord::from_sqlite_row)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|record| RepositoryTieredBackfillRecord {
                observed_at_unix_seconds: record.observed_at_unix_seconds,
                source_node_id: record.source_node_id,
                source_epoch: record.source_epoch,
                stream: record.stream,
                sequence: record.sequence,
                subject_node_id: record.subject_node_id,
                observer_node_id: record.observer_node_id,
                schema_id: record.schema_id,
                schema_version: record.schema_version,
                record_key: record.record_key,
                payload: record.payload,
                tombstone: record.tombstone,
            })
            .collect();
        Ok(RepositoryTieredBackfillPage {
            records,
            next_cursor,
        })
    }

    /// Import canonical rows from a ready repository's authenticated long-term export. The
    /// original source cursor remains the storage identity; this is intentionally separate from
    /// signed-frame receipt because compacted tiers no longer retain the original wire framing.
    pub(crate) fn import_tiered_backfill_records(
        &mut self,
        records: Vec<RepositoryTieredBackfillRecord>,
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<(), RepositoryRuntimeError> {
        if records.is_empty() {
            return Ok(());
        }
        self.refresh_capacity()?;
        let availability = self.snapshot.capacity.history_write_availability();
        if !availability.allows_history_writes() {
            return Err(RepositoryRuntimeError::WriteStopped(availability));
        }
        let previous_snapshot = self.snapshot.clone();
        let mut mutation = PendingRepositoryMutation::default();
        for record in records {
            let observed_at_unix_seconds = record.observed_at_unix_seconds;
            let cursor = ReplicaCursor::new(
                record.source_node_id.clone(),
                record.source_epoch,
                record.stream.clone(),
                record.sequence,
            )?;
            let sync_record = SyncRecord::new(
                record.subject_node_id,
                record.observer_node_id,
                record.schema_id,
                record.schema_version,
                record.record_key,
                record.payload,
                record.tombstone,
            );
            let replica_record = ReplicaRecord::new(
                &cursor,
                sync_record.subject_node_id(),
                sync_record.observer_node_id(),
                sync_record.schema().0,
                sync_record.schema().1,
                sync_record.record_key().to_vec(),
                sync_record.payload_bytes().to_vec(),
            )?;
            let key = replica_record.key();
            if sync_record.is_tombstone() {
                let (schema_id, schema_version) = sync_record.schema();
                let target_stream = source_stream_for_schema(schema_id).ok_or_else(|| {
                    RepositoryRuntimeError::Storage(format!(
                        "tombstone schema has no target stream: {schema_id}"
                    ))
                })?;
                let target_cursor = ReplicaCursor::new(
                    cursor.source_node_id(),
                    cursor.source_epoch(),
                    target_stream,
                    cursor.sequence(),
                )?;
                let target_key = ReplicaRecord::new(
                    &target_cursor,
                    sync_record.subject_node_id(),
                    sync_record.observer_node_id(),
                    schema_id,
                    schema_version,
                    sync_record.record_key().to_vec(),
                    sync_record.payload_bytes().to_vec(),
                )?
                .key();
                self.delete_records_for_tombstone(&target_key, &mut mutation)?;
                self.tombstones
                    .tombstone(key, now_unix_seconds, ready_repositories)?;
                self.tombstones
                    .acknowledge(replica_record.key(), local_repository_id)?;
            } else if !self.tombstones.allows(&key) {
                self.snapshot = previous_snapshot;
                self.tombstones =
                    TombstoneLedger::from_checkpoint(self.snapshot.tombstones.clone())?;
                return Err(RepositoryRuntimeError::Protocol(
                    ProtocolError::ResurrectionPrevented,
                ));
            }
            let stored = StoredRecord::from_record(
                observed_at_unix_seconds,
                now_unix_seconds,
                &cursor,
                &sync_record,
            );
            if self.uses_sqlite_history() {
                mutation.records.push(stored.sqlite_row()?);
            } else {
                self.snapshot.records.push(stored);
            }
            self.record_source_received_from_cursor(&cursor, now_unix_seconds);
        }
        self.persist_import_mutation(previous_snapshot, mutation)
    }
}
