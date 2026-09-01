use crate::history_sync::SignedSegment;
use crate::state::history_storage::{
    SourceDeliveryJournalPage, SourceDeliveryJournalRepairProgress, SourceDeliveryJournalRow,
};

use super::*;

impl RepositoryReplicaRuntime {
    pub(crate) fn hydrate_source_delivery_journal(
        &mut self,
    ) -> Result<bool, RepositoryRuntimeError> {
        if !self.storage.is_sqlite() {
            return Ok(true);
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
        // Keep the in-memory replay window bounded. Acknowledgement removes the durable head
        // before calling this method again, so the next page entry slides into the window on the
        // following delivery tick without loading an unbounded backlog into the control snapshot.
        let (rows, order_repairing) = match self
            .storage
            .source_delivery_journal_page(256)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
        {
            SourceDeliveryJournalPage::Ready(rows) => (rows, false),
            SourceDeliveryJournalPage::Repairing => (Vec::new(), true),
        };
        if order_repairing {
            return Ok(false);
        }
        let max_epoch = self
            .storage
            .source_delivery_journal_max_epoch()
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        // A control snapshot can be lost while the delivery journal survives. Start new source
        // records in a fresh epoch so the replayed journal and new observations cannot reuse a
        // cursor range. The durable epoch metadata is advanced by the next normal checkpoint.
        if self.snapshot.local_source.epoch == 0
            && let Some(max_epoch) = max_epoch
        {
            let next_epoch = max_epoch
                .checked_add(1)
                .filter(|epoch| *epoch <= i64::MAX as u64)
                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
            self.snapshot.local_source.epoch = next_epoch.max(1);
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
        Ok(true)
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
        let summary = if self.storage.is_sqlite() {
            self.storage
                .source_delivery_journal_summary()
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
        } else {
            crate::state::history_storage::SourceDeliveryJournalSummary {
                pending_segments: 0,
                pending_bytes: 0,
                oldest: None,
                last_acknowledged_at: None,
                last_delivery_path: None,
                order_repairing: false,
            }
        };
        let oldest_pending_age_seconds = summary
            .oldest
            .as_ref()
            .map(|row| now_unix_seconds.saturating_sub(row.closed_at_unix_seconds));
        let oldest_pending_cursor = summary.oldest.as_ref().and_then(|row| {
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
        } else if summary.order_repairing {
            "journal_order_repairing"
        } else if summary.pending_segments == 0 {
            "idle"
        } else {
            "backlogged"
        };
        Ok(SourceDeliveryStatus {
            state: state.to_owned(),
            pending_segments: summary.pending_segments,
            pending_bytes: summary.pending_bytes,
            oldest_pending_cursor,
            oldest_pending_age_seconds,
            last_acknowledged_at: summary.last_acknowledged_at,
            last_delivery_path: summary.last_delivery_path,
        })
    }

    pub(crate) fn repair_source_delivery_journal_order_page(
        &mut self,
    ) -> Result<SourceDeliveryJournalRepairProgress, RepositoryRuntimeError> {
        if !self.storage.is_sqlite() {
            return Ok(SourceDeliveryJournalRepairProgress {
                processed: 0,
                completed: true,
            });
        }
        self.storage
            .repair_source_delivery_journal_order_page()
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
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
