use crate::history_sync::SignedSegment;
use crate::state::history_storage::{
    SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS, SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES,
    SourceDeliveryJournalPage, SourceDeliveryJournalRepairProgress, SourceDeliveryJournalRow,
};
use std::collections::BTreeSet;

use super::*;

#[path = "source_journal_ack.rs"]
mod source_journal_ack;

impl LocalSourceState {
    pub(crate) fn rotate_after_repository_rebuild(&mut self) -> Result<(), RepositoryRuntimeError> {
        if self.epoch != 0 {
            if self.epoch >= i64::MAX as u64 {
                return Err(RepositoryRuntimeError::Storage(
                    "source epoch exhausted".to_owned(),
                ));
            }
            self.epoch += 1;
        }
        self.streams.clear();
        self.backpressure_gaps.clear();
        self.backpressure_gap_cursor = None;
        self.replay_window_cursor = None;
        self.deletion_marker_keys.clear();
        self.primary_failure_cycles = 0;
        self.standby_success_cycles = 0;
        self.primary_failure_repository_id = None;
        Ok(())
    }
}

impl RepositoryReplicaRuntime {
    pub(crate) fn finish_source_delivery_capture(
        &mut self,
        journal_ready: bool,
        hydration: Result<(), RepositoryRuntimeError>,
    ) -> Vec<RepositoryReplicaSegment> {
        if let Err(error) = hydration {
            // The journal transaction already committed. Keep durable rows as the source of
            // truth and clear only the in-memory window to avoid recapture.
            tracing::warn!(
                error = %error,
                "source delivery window hydration deferred after commit"
            );
            self.snapshot.local_source.clear_pending();
            return Vec::new();
        }
        if journal_ready {
            self.local_source_pending_segments_page()
        } else {
            Vec::new()
        }
    }

    pub(super) fn source_delivery_journal_has_unloaded_tail(
        &self,
    ) -> Result<bool, RepositoryRuntimeError> {
        let pending_by_stream = self
            .snapshot
            .local_source
            .streams
            .iter()
            .map(|(stream, state)| (stream.clone(), state.pending.len()))
            .collect::<BTreeMap<_, _>>();
        self.storage
            .source_delivery_journal_has_unloaded_tail(&pending_by_stream)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
    }

    pub(crate) fn local_source_backpressure_gaps(
        &mut self,
        source_node_id: &str,
    ) -> Vec<RepositoryReplicaGap> {
        self.local_source_gaps_for_segments(source_node_id, &[])
    }

    pub(crate) fn local_source_gaps_for_segments(
        &mut self,
        source_node_id: &str,
        pending_segments: &[RepositoryReplicaSegment],
    ) -> Vec<RepositoryReplicaGap> {
        const MAX_SOURCE_GAPS_PER_REQUEST: usize = 64;
        let keys = self
            .snapshot
            .local_source
            .backpressure_gaps
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        if keys.is_empty() {
            self.snapshot.local_source.backpressure_gap_cursor = None;
            return Vec::new();
        }
        // Keep the gap immediately before each pending segment in the bounded page so a receiver
        // can advance its signed chain without waiting for a full rotation of the gap cursor.
        let required_key_for_segment = |segment: &RepositoryReplicaSegment| {
            let signed = SignedSegment::from_wire(&segment.wire).ok()?;
            let cursor = signed.canonical().first_cursor();
            self.snapshot
                .local_source
                .backpressure_gaps
                .iter()
                .find(|(key, gap)| {
                    super::backpressure_gap_stream(key) == cursor.stream()
                        && gap.source_epoch == cursor.source_epoch()
                        && gap.last_sequence.checked_add(1) == Some(cursor.sequence())
                })
                .map(|(key, _)| key.clone())
        };
        let first_required_key = pending_segments.first().and_then(required_key_for_segment);
        let required_keys = pending_segments
            .iter()
            .filter_map(required_key_for_segment)
            .collect::<BTreeSet<_>>();
        let start = self
            .snapshot
            .local_source
            .backpressure_gap_cursor
            .as_ref()
            .and_then(|cursor| keys.iter().position(|key| key == cursor))
            .map_or(0, |index| (index + 1) % keys.len());
        let page_len = keys.len().min(MAX_SOURCE_GAPS_PER_REQUEST);
        let mut selected_keys = Vec::with_capacity(page_len);
        // The worker attaches the gap page to its first segment only. Preserve that segment's
        // predecessor even when many later pending segments contribute lexicographically earlier
        // required gaps.
        if let Some(key) = first_required_key.filter(|key| keys.binary_search(key).is_ok()) {
            selected_keys.push(key);
        }
        for key in required_keys {
            if selected_keys.len() >= page_len {
                break;
            }
            if keys.binary_search(&key).is_ok()
                && !selected_keys.iter().any(|selected| selected == &key)
            {
                selected_keys.push(key);
            }
        }
        for offset in 0..keys.len() {
            if selected_keys.len() >= page_len {
                break;
            }
            let key = &keys[(start + offset) % keys.len()];
            if !selected_keys.iter().any(|selected| selected == key) {
                selected_keys.push(key.clone());
            }
        }
        selected_keys
            .into_iter()
            .filter_map(|key| {
                self.snapshot
                    .local_source
                    .backpressure_gaps
                    .get(&key)
                    .map(|gap| RepositoryReplicaGap {
                        source_node_id: source_node_id.to_owned(),
                        source_epoch: gap.source_epoch,
                        stream: super::backpressure_gap_stream(&key).to_owned(),
                        first_sequence: gap.first_sequence,
                        last_sequence: gap.last_sequence,
                        start_unix_seconds: gap.start_unix_seconds,
                        end_unix_seconds: gap.end_unix_seconds,
                        permanent: true,
                        reason: None,
                    })
            })
            .collect()
    }

    pub(crate) fn commit_local_source_gap_page(
        &mut self,
        gaps: &[RepositoryReplicaGap],
    ) -> Result<(), RepositoryRuntimeError> {
        let mut committed_key = None;
        for gap in gaps {
            if let Some((key, _)) =
                self.snapshot
                    .local_source
                    .backpressure_gaps
                    .iter()
                    .find(|(key, candidate)| {
                        super::backpressure_gap_stream(key) == gap.stream
                            && candidate.source_epoch == gap.source_epoch
                            && candidate.first_sequence == gap.first_sequence
                            && candidate.last_sequence == gap.last_sequence
                    })
            {
                committed_key = Some(key.clone());
            }
        }
        if let Some(key) = committed_key {
            self.snapshot.local_source.backpressure_gap_cursor = Some(key);
            self.persist_control_state()?;
        }
        Ok(())
    }

    pub(crate) fn local_source_pending_segments_page(&self) -> Vec<RepositoryReplicaSegment> {
        self.local_source_pending_segments_page_with_budget(256, 1024 * 1024)
    }

    pub(crate) fn local_source_pending_segments_page_with_budget(
        &self,
        max_segments: usize,
        max_wire_bytes: usize,
    ) -> Vec<RepositoryReplicaSegment> {
        let mut streams = self
            .snapshot
            .local_source
            .streams
            .iter()
            .collect::<Vec<_>>();
        let tombstone = streams
            .iter()
            .find(|(stream, _)| stream.as_str() == "tombstone")
            .copied();
        let mut live_streams = streams
            .drain(..)
            .filter(|(stream, _)| stream.as_str() != "tombstone")
            .collect::<Vec<_>>();
        live_streams.sort_by_key(|(stream, _)| *stream);
        let start = self
            .snapshot
            .local_source
            .replay_window_cursor
            .as_deref()
            .and_then(|cursor| {
                live_streams
                    .iter()
                    .position(|(stream, _)| stream == &cursor)
            })
            .map_or(0, |index| (index + 1) % live_streams.len().max(1));
        let mut ordered_streams = Vec::with_capacity(live_streams.len() + 1);
        if let Some(tombstone) = tombstone {
            ordered_streams.push(tombstone);
        }
        ordered_streams.extend(
            (0..live_streams.len())
                .map(|offset| live_streams[(start + offset) % live_streams.len()]),
        );
        let mut page = Vec::new();
        let mut wire_bytes = 0_usize;
        let mut offsets = vec![0_usize; ordered_streams.len()];
        loop {
            if page.len() == max_segments {
                break;
            }
            let mut added = false;
            for (index, (_, state)) in ordered_streams.iter().enumerate() {
                let Some(pending) = state.pending.get(offsets[index]) else {
                    continue;
                };
                let next_wire_bytes = wire_bytes.saturating_add(pending.wire.len());
                if next_wire_bytes > max_wire_bytes {
                    continue;
                }
                wire_bytes = next_wire_bytes;
                offsets[index] += 1;
                page.push(RepositoryReplicaSegment {
                    identity: pending.identity.clone(),
                    wire: pending.wire.clone(),
                });
                added = true;
                if page.len() == max_segments {
                    break;
                }
            }
            if !added {
                break;
            }
        }
        page
    }

    pub(crate) fn clear_local_source_pending_window(&mut self) {
        self.snapshot.local_source.clear_pending();
    }

    pub(crate) fn source_delivery_capture_paused(&self) -> Result<bool, RepositoryRuntimeError> {
        if !self.storage.is_sqlite() {
            return Ok(false);
        }
        let summary = self
            .storage
            .source_delivery_journal_summary()
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let at_suspend_threshold = summary.pending_segments.saturating_mul(100)
            >= crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_MAX_SEGMENTS.saturating_mul(
                usize::try_from(
                    crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_SUSPEND_PERCENT,
                )
                .unwrap_or(80),
            )
            || summary.pending_bytes.saturating_mul(100)
                >= crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_MAX_BYTES.saturating_mul(
                    u64::try_from(
                        crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_SUSPEND_PERCENT,
                    )
                    .unwrap_or(80),
                );
        if summary.capacity_suspended || at_suspend_threshold {
            return Ok(true);
        }
        let available = self
            .storage
            .available_bytes()
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        Ok(available < 256 * 1024 * 1024)
    }

    pub(super) fn ensure_source_delivery_capacity(
        &self,
        defer_journal: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        if defer_journal || !self.storage.is_sqlite() {
            return Ok(());
        }
        if self
            .storage
            .source_delivery_journal_capacity_suspended()
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
        {
            return Err(RepositoryRuntimeError::Storage(
                "source delivery journal capacity guard".to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn hydrate_source_delivery_journal(
        &mut self,
    ) -> Result<bool, RepositoryRuntimeError> {
        self.hydrate_source_delivery_journal_with_budget(
            SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS,
            SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES,
        )
    }

    pub(crate) fn hydrate_source_delivery_journal_with_budget(
        &mut self,
        max_segments: usize,
        max_wire_bytes: usize,
    ) -> Result<bool, RepositoryRuntimeError> {
        if !self.storage.is_sqlite() {
            return Ok(true);
        }
        let had_pending_window = self
            .snapshot
            .local_source
            .streams
            .values()
            .any(|stream| !stream.pending.is_empty());
        let pending_window_segments = self
            .snapshot
            .local_source
            .streams
            .values()
            .map(|stream| stream.pending.len())
            .sum::<usize>();
        let pending_window_wire_bytes = self
            .snapshot
            .local_source
            .streams
            .values()
            .flat_map(|stream| stream.pending.iter())
            .map(|segment| segment.wire.len())
            .sum::<usize>();
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
        if !legacy_rows.is_empty() {
            self.storage
                .append_source_delivery_journal(&legacy_rows)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        }
        // Keep an unacknowledged replay page stable across failed delivery cycles. A new page is
        // selected only after ACK removes the current window, which preserves source ordering and
        // prevents a transient collector failure from rotating past an unresolved sequence gap.
        if had_pending_window
            && pending_window_segments <= SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS
            && pending_window_wire_bytes <= SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES
        {
            let summary = self
                .storage
                .source_delivery_journal_summary()
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            if summary.order_repairing {
                self.snapshot.local_source.clear_pending();
                return Ok(false);
            }
            return Ok(true);
        }
        // A legacy snapshot may contain an unbounded pre-journal queue. It has already been
        // copied into SQLite above, so drop the oversized in-memory copy before rebuilding a
        // bounded replay window.
        if had_pending_window {
            self.snapshot.local_source.clear_pending();
        }
        if !legacy_rows.is_empty() {
            self.snapshot.local_source.clear_pending();
        }
        let stream_names = self
            .snapshot
            .local_source
            .streams
            .keys()
            .cloned()
            .chain(
                super::KNOWN_SCHEMAS
                    .iter()
                    .filter_map(|(schema, _)| super::stream_for_schema(schema))
                    .map(ToOwned::to_owned),
            )
            .collect::<BTreeSet<_>>();
        // Keep the in-memory replay window bounded. Acknowledgement removes the durable head
        // before calling this method again, so the next page entry slides into the window on the
        // following delivery tick without loading an unbounded backlog into the control snapshot.
        let stream_names = stream_names
            .into_iter()
            .chain(std::iter::once("tombstone".to_owned()))
            .collect::<Vec<_>>();
        let stream_heads = self
            .storage
            .source_delivery_journal_stream_heads(&stream_names, max_segments, max_wire_bytes)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let head_wire_bytes = stream_heads.iter().map(|row| row.wire.len()).sum::<usize>();
        let (rows, order_repairing) = match self
            .storage
            .source_delivery_journal_page_with_budget(
                max_segments.saturating_sub(stream_heads.len()),
                max_wire_bytes.saturating_sub(head_wire_bytes),
            )
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
            if let Some(row) = rows.first().or_else(|| stream_heads.first()) {
                self.snapshot.local_source.node_id = row.identity.node_id().as_str().to_owned();
            }
        }
        let next_replay_stream = stream_heads
            .iter()
            .rev()
            .find(|row| row.stream != "tombstone")
            .map(|row| row.stream.clone());
        for row in rows.into_iter().chain(stream_heads) {
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
        // A completed ACK rotates the bounded window in-process as well as across restarts. Keep
        // the cursor at the last stream selected for the new window so a hot stream cannot starve
        // later streams while the durable journal still contains their tails.
        self.snapshot.local_source.replay_window_cursor = next_replay_stream;
        Ok(true)
    }

    pub(super) fn local_source_pending_window_within_budget(&self) -> bool {
        let pending_segments = self
            .snapshot
            .local_source
            .streams
            .values()
            .map(|stream| stream.pending.len())
            .sum::<usize>();
        let pending_wire_bytes = self
            .snapshot
            .local_source
            .streams
            .values()
            .flat_map(|stream| stream.pending.iter())
            .map(|segment| segment.wire.len())
            .sum::<usize>();
        pending_segments <= SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS
            && pending_wire_bytes <= SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES
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
        self.source_delivery_status_with_caller(
            now_unix_seconds,
            storage_degraded,
            filesystem_available_bytes,
            "history_repository.source_delivery_status",
        )
    }

    pub(crate) fn source_delivery_status_with_caller(
        &self,
        now_unix_seconds: u64,
        storage_degraded: bool,
        filesystem_available_bytes: u64,
        caller_class: &'static str,
    ) -> Result<SourceDeliveryStatus, RepositoryRuntimeError> {
        let summary = if self.storage.is_sqlite() {
            self.storage
                .source_delivery_journal_summary_with_caller(caller_class)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
        } else {
            crate::state::history_storage::SourceDeliveryJournalSummary {
                pending_segments: 0,
                pending_bytes: 0,
                oldest: None,
                last_acknowledged_at: None,
                last_delivery_path: None,
                order_repairing: false,
                capacity_suspended: false,
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
        } else if summary.capacity_suspended {
            "journal_capacity_guard"
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
