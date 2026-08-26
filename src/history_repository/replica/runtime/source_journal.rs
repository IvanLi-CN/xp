use crate::history_sync::SignedSegment;
use crate::state::history_storage::SourceDeliveryJournalRow;

use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn hydrate_source_delivery_journal(&mut self) -> Result<(), RepositoryRuntimeError> {
        if !self.storage.is_sqlite() {
            return Ok(());
        }
        let legacy_rows = self
            .snapshot
            .local_source
            .streams
            .values()
            .flat_map(|stream| stream.pending.iter())
            .map(|segment| SourceDeliveryJournalRow {
                id: segment.id.clone(),
                stream: super::stream_for_wire(&segment.wire),
                closed_at_unix_seconds: segment.closed_at_unix_seconds,
                identity: segment.identity.clone(),
                wire: segment.wire.clone(),
            })
            .collect::<Vec<_>>();
        self.storage
            .append_source_delivery_journal(&legacy_rows)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if !legacy_rows.is_empty() {
            self.snapshot.local_source.clear_pending();
        }
        let max_epoch = self
            .storage
            .source_delivery_journal_max_epoch()
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let rows = self
            .storage
            .source_delivery_journal_page(256)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        // A control snapshot can be lost while the delivery journal survives. Start new source
        // records in a fresh epoch so the replayed journal and new observations cannot reuse a
        // cursor range. The durable epoch metadata is advanced by the next normal checkpoint.
        if self.snapshot.local_source.epoch == 0
            && let Some(max_epoch) = max_epoch
        {
            self.snapshot.local_source.epoch = max_epoch.saturating_add(1).max(1);
            if let Some(row) = rows.first() {
                self.snapshot.local_source.node_id = row.identity.node_id().as_str().to_owned();
            }
        }
        for row in rows {
            let stream = row.stream;
            let segment = StoredSegment {
                id: row.id,
                closed_at_unix_seconds: row.closed_at_unix_seconds,
                identity: row.identity,
                wire: row.wire,
            };
            if self
                .snapshot
                .local_source
                .streams
                .values()
                .any(|state| state.pending.iter().any(|pending| pending.id == segment.id))
            {
                continue;
            }
            self.snapshot
                .local_source
                .streams
                .entry(stream)
                .or_default()
                .pending
                .push_back(segment);
        }
        Ok(())
    }

    pub(crate) fn snapshot_for_persistence(&self) -> RepositoryReplicaSnapshot {
        let mut snapshot = self.snapshot.clone();
        if self.storage.is_sqlite() {
            snapshot.local_source.clear_pending();
        }
        snapshot
    }

    pub(crate) fn source_delivery_status(
        &self,
        now_unix_seconds: u64,
        storage_degraded: bool,
        filesystem_available_bytes: u64,
    ) -> Result<SourceDeliveryStatus, RepositoryRuntimeError> {
        let rows = if self.storage.is_sqlite() {
            self.storage
                .source_delivery_journal()
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
        } else {
            Vec::new()
        };
        let pending_bytes = rows
            .iter()
            .map(|row| u64::try_from(row.wire.len()).unwrap_or(u64::MAX))
            .sum();
        let oldest_pending_age_seconds = rows
            .first()
            .map(|row| now_unix_seconds.saturating_sub(row.closed_at_unix_seconds));
        let oldest_pending_cursor = rows.first().and_then(|row| {
            SignedSegment::from_wire(&row.wire).ok().map(|segment| {
                let cursor = segment.canonical().first_cursor();
                format!(
                    "{}/{}/{}/{}",
                    cursor.source_node_id(),
                    cursor.source_epoch(),
                    cursor.stream(),
                    cursor.sequence()
                )
            })
        });
        let state = if storage_degraded || !self.storage.is_sqlite() {
            "journal_unavailable"
        } else if filesystem_available_bytes < 256 * 1024 * 1024 {
            "source_storage_guard"
        } else if rows.is_empty() {
            "idle"
        } else {
            "backlogged"
        };
        Ok(SourceDeliveryStatus {
            state: state.to_owned(),
            pending_segments: rows.len(),
            pending_bytes,
            oldest_pending_cursor,
            oldest_pending_age_seconds,
            last_acknowledged_at: None,
            last_delivery_path: None,
        })
    }

    pub(crate) fn persist_control_state_with_journal(
        &mut self,
        journal_rows: &[SourceDeliveryJournalRow],
    ) -> Result<(), RepositoryRuntimeError> {
        self.snapshot.tombstones = self.tombstones.checkpoint();
        let bytes = serde_json::to_vec(&self.snapshot_for_persistence())
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if bytes.len() > MAX_RUNTIME_STATE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let result = self
            .storage
            .append_source_delivery_journal_and_control(journal_rows, &bytes);
        self.finish_storage_write(result)
    }
}
