use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, VecDeque};

use crate::state::{
    history_repository::replica::RepositoryReplicaGap, history_storage::SourceDeliveryJournalRow,
};

use super::*;
#[path = "source_journal.rs"]
mod source_journal;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct LocalSourceState {
    #[serde(default)]
    epoch: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    node_id: String,
    #[serde(default)]
    streams: BTreeMap<String, LocalSourceStreamState>,
    #[serde(default)]
    last_dynamic_relay_attempt_unix_seconds: Option<u64>,
    #[serde(default)]
    primary_failure_cycles: u8,
    #[serde(default)]
    standby_success_cycles: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    primary_failure_repository_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    backpressure_gaps: BTreeMap<String, LocalSourceGap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    backpressure_gap_cursor: Option<String>,
    #[serde(skip)]
    replay_window_cursor: Option<String>,
    /// Durable marker-to-cursor mapping. Tombstones use their own stream, so their sequence
    /// cannot be reconstructed from the affected schema's live cursor.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    deletion_marker_keys: BTreeMap<String, ReplicaRecordKey>,
}
struct LocalSourceQueueOptions<'a> {
    ready_repositories: &'a [String],
    _max_pending_segments_per_stream: usize,
    persist: bool,
    defer_journal: bool,
}
impl LocalSourceState {
    pub(super) fn stream_checkpoint(&self, stream: &str) -> Option<(u64, Option<[u8; 32]>)> {
        self.streams
            .get(stream)
            .map(|state| (state.next_sequence, state.previous_segment_hash))
    }

    pub(super) fn has_never_emitted_tombstones(&self) -> bool {
        self.streams
            .get("tombstone")
            .is_none_or(|state| state.next_sequence == 0 && state.previous_segment_hash.is_none())
            && self.deletion_marker_keys.is_empty()
            && !self.backpressure_gaps.contains_key("tombstone")
    }

    pub(super) fn has_tombstone_backpressure_gap(&self) -> bool {
        self.backpressure_gaps.contains_key("tombstone")
    }

    pub(super) fn epoch(&self) -> u64 {
        self.epoch
    }
    pub(super) fn node_id(&self) -> Option<&str> {
        (!self.node_id.is_empty()).then_some(self.node_id.as_str())
    }
    fn clear_pending(&mut self) {
        for stream in self.streams.values_mut() {
            stream.pending.clear();
        }
    }

    pub(super) fn pending_segment_by_id(&self, segment_id: &str) -> Option<&StoredSegment> {
        self.streams
            .values()
            .flat_map(|stream| stream.pending.iter())
            .find(|segment| segment.id == segment_id)
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LocalSourceStreamState {
    #[serde(default)]
    next_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_segment_hash: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "VecDeque::is_empty")]
    pending: VecDeque<StoredSegment>,
    #[serde(default)]
    backpressured_records: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalSourceGap {
    source_epoch: u64,
    first_sequence: u64,
    last_sequence: u64,
    start_unix_seconds: u64,
    end_unix_seconds: u64,
}
impl RepositoryReplicaRuntime {
    pub(super) fn validate_existing_tombstone_receiver(
        &self,
        identity: &RepositoryNodeIdentity,
        epoch: u64,
        end: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        let Some(receiver) = self.receiver.as_ref() else {
            return Ok(());
        };
        let checkpoint = serde_json::to_value(receiver.checkpoint()?)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let streams = checkpoint["streams"]
            .as_array()
            .ok_or_else(|| RepositoryRuntimeError::Storage("invalid receiver checkpoint".into()))?;
        for entry in streams {
            if entry["key"]["source_node_id"].as_str() != Some(identity.node_id().as_str())
                || entry["key"]["stream"].as_str() != Some("tombstone")
            {
                continue;
            }
            let progress = &entry["progress"];
            if progress["epoch"].as_u64() != Some(epoch)
                || progress["hash_chain_verified"].as_bool() != Some(true)
                || progress["last_sequence"]
                    .as_u64()
                    .is_none_or(|sequence| sequence >= end)
            {
                return Err(RepositoryRuntimeError::Storage(
                    "existing tombstone receiver is unverified or beyond the captured source tail"
                        .into(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn set_local_source_epoch(&mut self, epoch: u64, node_id: &str) {
        self.snapshot.local_source.epoch = epoch;
        self.snapshot.local_source.node_id = node_id.to_owned();
    }

    pub(crate) fn history_truncated(&self) -> bool {
        self.snapshot.history_truncated
    }
    pub(crate) fn can_accept_retained_sequence_gap(
        &self,
        wire: &[u8],
        already_consumed: bool,
    ) -> Result<bool, RepositoryRuntimeError> {
        let segment = crate::history_sync::SignedSegment::from_wire(wire)?;
        let first = segment.canonical().first_cursor();
        let Some(receiver) = self.receiver.as_ref() else {
            return Ok(false);
        };
        let Some(watermark) = receiver.continuous_watermark(first)? else {
            return Ok(false);
        };
        let expected = watermark.sequence().saturating_add(1);
        if already_consumed
            || first.sequence() <= expected
            || segment.canonical().previous_segment_hash().is_none()
        {
            return Ok(false);
        }
        Ok(true)
    }
    pub(crate) fn tiered_handoff_for_sequence_gap(
        &self,
        wire: &[u8],
    ) -> Result<Option<InitialPeerTieredHandoff>, RepositoryRuntimeError> {
        let segment = crate::history_sync::SignedSegment::from_wire(wire)?;
        let first = segment.canonical().first_cursor();
        let Some(receiver) = self.receiver.as_ref() else {
            return Ok(None);
        };
        let Some(watermark) = receiver.continuous_watermark(first)? else {
            return Ok(None);
        };
        let Some(first_missing) = watermark.sequence().checked_add(1) else {
            return Ok(None);
        };
        // The receiver already has a predecessor watermark, so a retained anchor does not need
        // to carry its predecessor hash. The handoff records the omitted range before retrying
        // the same anchor through the retained-anchor receive path.
        if first.sequence() <= first_missing {
            return Ok(None);
        }
        if self.snapshot.gaps.iter().any(|gap| {
            gap.permanent
                && gap.source_node_id == first.source_node_id()
                && gap.source_epoch == first.source_epoch()
                && gap.stream == first.stream()
                && gap.first_sequence <= first_missing
                && gap.last_sequence >= first.sequence().saturating_sub(1)
        }) {
            return Ok(None);
        }
        Ok(Some(InitialPeerTieredHandoff {
            source_node_id: first.source_node_id().to_owned(),
            source_epoch: first.source_epoch(),
            stream: first.stream().to_owned(),
            first_missing,
            last_missing: first.sequence().saturating_sub(1),
            next_sequence: first.sequence(),
            end_unix_seconds: segment.canonical().opened_at_unix_seconds(),
        }))
    }
    pub(crate) fn queue_local_source_segments(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: &[SyncRecord],
        now_unix_seconds: u64,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        self.queue_local_source_segments_for_repositories(
            cluster_id,
            identity,
            signing_key,
            records.iter().cloned(),
            now_unix_seconds,
            &["local".to_owned()],
        )
    }
    pub(crate) fn queue_local_source_segments_for_repositories(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: impl IntoIterator<Item = SyncRecord>,
        now_unix_seconds: u64,
        ready_repositories: &[String],
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        self.queue_local_source_segments_for_repositories_with_limit(
            cluster_id,
            identity,
            signing_key,
            records,
            now_unix_seconds,
            LocalSourceQueueOptions {
                ready_repositories,
                _max_pending_segments_per_stream: usize::MAX,
                persist: true,
                defer_journal: false,
            },
        )
    }

    fn queue_local_source_segments_for_repositories_with_limit(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: impl IntoIterator<Item = SyncRecord>,
        now_unix_seconds: u64,
        options: LocalSourceQueueOptions<'_>,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        let journal_ready = self.hydrate_source_delivery_journal()?;
        let journal_has_unloaded_tail = self.source_delivery_journal_has_unloaded_tail()?;
        let previous_snapshot = self.snapshot.clone();
        let previous_tombstones = self.tombstones.checkpoint();
        let mut records_by_stream = BTreeMap::<&'static str, Vec<SyncRecord>>::new();
        for record in records {
            if record.is_tombstone()
                && self
                    .snapshot
                    .local_source
                    .deletion_marker_keys
                    .contains_key(&deletion_marker_id(&record))
            {
                continue;
            }
            let (schema_id, _) = record.schema();
            let stream = if record.is_tombstone() {
                "tombstone"
            } else {
                stream_for_schema(schema_id).ok_or_else(|| {
                    RepositoryRuntimeError::Storage(format!(
                        "history source schema has no independent stream: {schema_id}"
                    ))
                })?
            };
            records_by_stream.entry(stream).or_default().push(record);
        }
        if records_by_stream.is_empty() {
            return Ok(if journal_ready {
                self.local_source_pending_segments_page()
            } else {
                Vec::new()
            });
        }
        if self.storage.is_sqlite() {
            let available = self
                .storage
                .available_bytes()
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            if available < 256 * 1024 * 1024 {
                return Err(RepositoryRuntimeError::WriteStopped(
                    HistoryWriteAvailability::DegradedLowSpace,
                ));
            }
            self.ensure_source_delivery_capacity(options.defer_journal)?;
        }
        if self.storage.is_sqlite() && !journal_ready && self.snapshot.local_source.epoch == 0 {
            return Err(RepositoryRuntimeError::Storage(
                "source journal order repair must finish before source epoch recovery".to_owned(),
            ));
        }
        if self.snapshot.local_source.epoch == 0 {
            let result = self.storage.allocate_repository_source_epoch(
                cluster_id,
                identity.node_id().as_str(),
                source_epoch(cluster_id, identity.node_id().as_str()),
            );
            self.snapshot.local_source.epoch = self.finish_storage_write(result)?;
            self.snapshot.local_source.node_id = identity.node_id().as_str().to_owned();
        }
        let mut journal_rows = Vec::new();
        for (stream, records) in records_by_stream {
            let record_count = u64::try_from(records.len())
                .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?;
            let next_sequence = self
                .snapshot
                .local_source
                .streams
                .entry(stream.to_owned())
                .or_default()
                .next_sequence;
            for (offset, record) in records.iter().enumerate() {
                if !record.is_tombstone() {
                    continue;
                }
                let sequence = next_sequence
                    .checked_add(
                        u64::try_from(offset)
                            .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
                    )
                    .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
                let cursor = ReplicaCursor::new(
                    identity.node_id().as_str(),
                    self.snapshot.local_source.epoch,
                    "tombstone",
                    sequence,
                )?;
                let key = ReplicaRecord::new(
                    &cursor,
                    record.subject_node_id(),
                    record.observer_node_id(),
                    record.schema().0,
                    record.schema().1,
                    record.record_key().to_vec(),
                    record.payload_bytes().to_vec(),
                )?
                .key();
                self.tombstones.tombstone(
                    key.clone(),
                    now_unix_seconds,
                    options.ready_repositories,
                )?;
                let (schema_id, _) = record.schema();
                let generation = deletion_marker_generation(record);
                let legacy_id = deletion_marker_id_parts(schema_id, record.record_key(), 0);
                let prefix = format!("{legacy_id}:generation:");
                self.snapshot
                    .local_source
                    .deletion_marker_keys
                    .retain(|id, _| {
                        if generation == 0 {
                            return true;
                        }
                        if id == &legacy_id {
                            return false;
                        }
                        id.strip_prefix(&prefix)
                            .and_then(|value| value.parse::<u64>().ok())
                            .is_none_or(|existing| existing >= generation)
                    });
                self.snapshot
                    .local_source
                    .deletion_marker_keys
                    .insert(deletion_marker_id(record), key);
            }
            let stream_state = self
                .snapshot
                .local_source
                .streams
                .get_mut(stream)
                .expect("source stream was initialized");
            let first_cursor = Cursor::new(
                identity.node_id().as_str(),
                self.snapshot.local_source.epoch,
                stream,
                stream_state.next_sequence,
            )?;
            let signed = CanonicalSegment::new(
                cluster_id,
                first_cursor,
                records,
                stream_state.previous_segment_hash,
                now_unix_seconds,
                now_unix_seconds,
            )?
            .sign(signing_key)?;
            let wire = signed.wire_bytes()?;
            stream_state.next_sequence = stream_state
                .next_sequence
                .checked_add(record_count)
                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
            stream_state.previous_segment_hash = Some(signed.segment_hash()?);
            let stored = StoredSegment {
                id: hex::encode(Sha256::digest(&wire)),
                closed_at_unix_seconds: now_unix_seconds,
                identity: identity.clone(),
                wire: wire.clone(),
            };
            journal_rows.push(SourceDeliveryJournalRow {
                id: stored.id.clone(),
                stream: stream.to_owned(),
                closed_at_unix_seconds: stored.closed_at_unix_seconds,
                identity: stored.identity.clone(),
                wire: stored.wire.clone(),
            });
            stream_state.pending.push_back(stored);
        }
        if self.storage.is_sqlite() && !options.defer_journal {
            if options.persist {
                if let Err(error) = self
                    .persist_control_state_with_journal(&journal_rows, options.ready_repositories)
                {
                    self.snapshot = previous_snapshot;
                    self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
                    return Err(error);
                }
            } else {
                let result = self.storage.append_source_delivery_journal(&journal_rows);
                if let Err(error) = self.finish_storage_write(result) {
                    self.snapshot = previous_snapshot;
                    self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
                    return Err(error);
                }
            }
        } else if options.persist
            && let Err(error) = self.persist_control_state()
        {
            self.snapshot = previous_snapshot;
            self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
            return Err(error);
        }
        let hydration = if options.defer_journal {
            Ok(())
        } else if journal_ready && journal_has_unloaded_tail {
            for (stream, state) in &mut self.snapshot.local_source.streams {
                state.pending = previous_snapshot
                    .local_source
                    .streams
                    .get(stream)
                    .map_or_else(VecDeque::new, |previous| previous.pending.clone());
            }
            Ok(())
        } else if journal_ready && self.local_source_pending_window_within_budget() {
            Ok(())
        } else {
            self.hydrate_source_delivery_journal().map(|_| ())
        };
        Ok(self.finish_source_delivery_capture(journal_ready, hydration))
    }
    pub(crate) fn queue_local_source_segment(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: Vec<SyncRecord>,
        now_unix_seconds: u64,
    ) -> Result<Option<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        Ok(self
            .queue_local_source_segments(
                cluster_id,
                identity,
                signing_key,
                &records,
                now_unix_seconds,
            )?
            .into_iter()
            .next())
    }

    pub(super) fn remove_local_source_pending_segment(
        &mut self,
        delivered_wire: &[u8],
    ) -> Result<bool, RepositoryRuntimeError> {
        for stream in self.snapshot.local_source.streams.values_mut() {
            let Some(index) = stream
                .pending
                .iter()
                .position(|pending| pending.wire == delivered_wire)
            else {
                continue;
            };
            if index != 0 {
                return Err(RepositoryRuntimeError::Storage(
                    "source delivery acknowledgement arrived out of order".to_owned(),
                ));
            }
            {
                stream.pending.pop_front();
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn begin_source_dynamic_relay_attempt(
        &mut self,
        cluster_id: &str,
        now_unix_seconds: u64,
    ) -> Result<bool, RepositoryRuntimeError> {
        let jitter_seconds = u64::from(Sha256::digest(cluster_id.as_bytes())[1]) % (5 * 60);
        let due = self
            .snapshot
            .local_source
            .last_dynamic_relay_attempt_unix_seconds
            .is_none_or(|last| now_unix_seconds.saturating_sub(last) >= 60 * 60 + jitter_seconds);
        if due {
            self.snapshot
                .local_source
                .last_dynamic_relay_attempt_unix_seconds = Some(now_unix_seconds);
            self.persist_control_state()?;
        }
        Ok(due)
    }

    pub(crate) fn local_source_pending_segments(&self) -> Vec<RepositoryReplicaSegment> {
        const MAX_SEGMENTS: usize =
            crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS;
        const MAX_WIRE_BYTES: usize =
            crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES;
        let mut pending = self
            .snapshot
            .local_source
            .streams
            .iter()
            .filter_map(|(stream, state)| state.pending.front().map(|pending| (stream, pending)))
            .collect::<Vec<_>>();
        // A deletion must reach every repository before a later record can resurrect the same
        // key, so the independent tombstone stream is always offered first.
        pending.sort_by_key(|(stream, _)| (*stream != "tombstone", *stream));
        let mut page = Vec::new();
        let mut wire_bytes = 0_usize;
        for (_, pending) in pending {
            if page.len() == MAX_SEGMENTS {
                break;
            }
            let next_wire_bytes = wire_bytes.saturating_add(pending.wire.len());
            if next_wire_bytes > MAX_WIRE_BYTES {
                continue;
            }
            wire_bytes = next_wire_bytes;
            page.push(RepositoryReplicaSegment {
                identity: pending.identity.clone(),
                wire: pending.wire.clone(),
            });
        }
        page
    }

    #[cfg(test)]
    pub(crate) fn local_source_next_sequence(&self, stream: &str) -> Option<u64> {
        self.snapshot
            .local_source
            .streams
            .get(stream)
            .map(|state| state.next_sequence)
    }

    #[cfg(test)]
    pub(crate) fn local_source_replay_window_cursor(&self) -> Option<&str> {
        self.snapshot.local_source.replay_window_cursor.as_deref()
    }

    pub(crate) fn local_source_tombstones_fully_acknowledged(
        &self,
        _source_node_id: &str,
        markers: &[crate::node_history::RepositoryHistoryDeletionMarker],
    ) -> Result<bool, RepositoryRuntimeError> {
        Ok(markers.iter().all(|marker| {
            self.snapshot
                .local_source
                .deletion_marker_keys
                .get(&deletion_marker_id_parts(
                    &marker.schema_id,
                    &marker.record_key,
                    marker.generation,
                ))
                .is_some_and(|key| self.tombstones.fully_acknowledged(key))
        }))
    }

    pub(crate) fn complete_local_source_tombstones(
        &mut self,
        markers: &[crate::node_history::RepositoryHistoryDeletionMarker],
    ) -> Result<(), RepositoryRuntimeError> {
        let previous = self.snapshot.local_source.clone();
        let mut changed = false;
        for marker in markers {
            changed |= self
                .snapshot
                .local_source
                .deletion_marker_keys
                .remove(&deletion_marker_id_parts(
                    &marker.schema_id,
                    &marker.record_key,
                    marker.generation,
                ))
                .is_some();
        }
        if changed && let Err(error) = self.persist_control_state() {
            self.snapshot.local_source = previous;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn local_source_collector(
        &self,
        primary_repository_id: &str,
        standby_repository_id: Option<&str>,
    ) -> String {
        if self
            .snapshot
            .local_source
            .primary_failure_repository_id
            .as_deref()
            == Some(primary_repository_id)
            && self.snapshot.local_source.primary_failure_cycles >= 3
            && let Some(standby_repository_id) = standby_repository_id
        {
            return standby_repository_id.to_owned();
        }
        primary_repository_id.to_owned()
    }
    pub(crate) fn record_local_source_collector_delivery(
        &mut self,
        primary_repository_id: &str,
        selected_repository_id: &str,
        succeeded: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        if self
            .snapshot
            .local_source
            .primary_failure_repository_id
            .as_deref()
            != Some(primary_repository_id)
        {
            self.snapshot.local_source.primary_failure_repository_id =
                Some(primary_repository_id.to_owned());
            self.snapshot.local_source.primary_failure_cycles = 0;
            self.snapshot.local_source.standby_success_cycles = 0;
        }
        if selected_repository_id != primary_repository_id {
            if succeeded {
                self.snapshot.local_source.standby_success_cycles = self
                    .snapshot
                    .local_source
                    .standby_success_cycles
                    .saturating_add(1);
                if self.snapshot.local_source.standby_success_cycles >= 3 {
                    self.snapshot.local_source.primary_failure_cycles = 0;
                    self.snapshot.local_source.standby_success_cycles = 0;
                }
            } else {
                self.snapshot.local_source.primary_failure_cycles = 0;
                self.snapshot.local_source.standby_success_cycles = 0;
            }
            return self.persist_control_state();
        }
        if succeeded {
            self.snapshot.local_source.primary_failure_cycles = 0;
            self.snapshot.local_source.standby_success_cycles = 0;
        } else {
            self.snapshot.local_source.standby_success_cycles = 0;
            self.snapshot.local_source.primary_failure_cycles = self
                .snapshot
                .local_source
                .primary_failure_cycles
                .saturating_add(1);
        }
        self.persist_control_state()
    }

    fn record_local_source_backpressure_gap(
        &mut self,
        stream: &str,
        first_sequence: u64,
        last_sequence: u64,
        now_unix_seconds: u64,
    ) {
        let epoch = self.snapshot.local_source.epoch;
        let contiguous_key = self
            .snapshot
            .local_source
            .backpressure_gaps
            .iter()
            .find(|(key, gap)| {
                backpressure_gap_stream(key) == stream
                    && (gap.last_sequence.checked_add(1) == Some(first_sequence)
                        || last_sequence.checked_add(1) == Some(gap.first_sequence))
            })
            .map(|(key, _)| key.clone());
        let key = contiguous_key.unwrap_or_else(|| format!("{stream}\0{first_sequence:020}"));
        let gap = self
            .snapshot
            .local_source
            .backpressure_gaps
            .entry(key)
            .or_insert(LocalSourceGap {
                source_epoch: epoch,
                first_sequence,
                last_sequence,
                start_unix_seconds: now_unix_seconds,
                end_unix_seconds: now_unix_seconds,
            });
        gap.first_sequence = gap.first_sequence.min(first_sequence);
        gap.last_sequence = gap.last_sequence.max(last_sequence);
        gap.start_unix_seconds = gap.start_unix_seconds.min(now_unix_seconds);
        gap.end_unix_seconds = gap.end_unix_seconds.max(now_unix_seconds);
    }
}

fn backpressure_gap_stream(key: &str) -> &str {
    key.split_once('\0').map_or(key, |(stream, _)| stream)
}

fn stream_for_schema(schema_id: &str) -> Option<&'static str> {
    Some(match schema_id {
        "runtime.v1" => "runtime",
        "path_health.v1" => "path_health",
        "traffic.v1" => "traffic",
        "connections.v1" => "connections",
        "ip_usage.v1" => "ip_usage",
        crate::uptime_monitor::UPTIME_HISTORY_SCHEMA => {
            crate::uptime_monitor::UPTIME_HISTORY_STREAM
        }
        crate::resource_monitoring::RESOURCE_HISTORY_SCHEMA => {
            crate::resource_monitoring::RESOURCE_HISTORY_STREAM
        }
        "tombstone.v1" => "tombstone",
        _ => return None,
    })
}

fn stream_for_wire(wire: &[u8]) -> String {
    SignedSegment::from_wire(wire)
        .ok()
        .map(|segment| segment.canonical().first_cursor().stream().to_owned())
        .unwrap_or_default()
}

fn deletion_marker_id(record: &SyncRecord) -> String {
    let (schema_id, _) = record.schema();
    deletion_marker_id_parts(
        schema_id,
        record.record_key(),
        deletion_marker_generation(record),
    )
}

fn deletion_marker_generation(record: &SyncRecord) -> u64 {
    serde_json::from_slice::<serde_json::Value>(record.payload_bytes())
        .ok()
        .and_then(|value| value["deletion_generation"].as_u64())
        .unwrap_or(0)
}

fn deletion_marker_id_parts(schema_id: &str, record_key: &[u8], generation: u64) -> String {
    let legacy = format!("{schema_id}:{}", hex::encode(record_key));
    if generation == 0 {
        legacy
    } else {
        format!("{legacy}:generation:{generation}")
    }
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "source_deletion_generation_tests.rs"]
mod deletion_generation_tests;
