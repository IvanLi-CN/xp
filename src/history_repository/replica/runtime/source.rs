use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

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

    pub(super) fn rotate_after_repository_rebuild(&mut self) -> Result<(), RepositoryRuntimeError> {
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
        self.deletion_marker_keys.clear();
        self.primary_failure_cycles = 0;
        self.standby_success_cycles = 0;
        self.primary_failure_repository_id = None;
        Ok(())
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
    const MAX_HISTORY_BACKFILL_PENDING_SEGMENTS_PER_STREAM: usize = 128;

    pub(crate) fn queue_local_source_segments(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: Vec<SyncRecord>,
        now_unix_seconds: u64,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        self.queue_local_source_segments_for_repositories(
            cluster_id,
            identity,
            signing_key,
            records,
            now_unix_seconds,
            &["local".to_owned()],
        )
    }

    /// Queue one historical backfill page without returning unrelated live outbox fronts. The
    /// caller can atomically acknowledge these exact segments with the page checkpoint.
    pub(crate) fn queue_local_history_backfill_segments(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: Vec<SyncRecord>,
        now_unix_seconds: u64,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        // A page checkpoint is all-or-nothing. Reject before queuing if any target stream has a
        // pending segment: its predecessor must be delivered first, and appending a backfill
        // segment behind it could not be atomically acknowledged with this page's checkpoint.
        for record in &records {
            let stream = if record.is_tombstone() {
                "tombstone"
            } else {
                stream_for_schema(record.schema().0).ok_or_else(|| {
                    RepositoryRuntimeError::Storage(format!(
                        "history source schema has no independent stream: {}",
                        record.schema().0
                    ))
                })?
            };
            if self
                .snapshot
                .local_source
                .streams
                .get(stream)
                .is_some_and(|state| !state.pending.is_empty())
            {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
        }
        let requires_segment = !records.is_empty();
        let pending_before = self
            .snapshot
            .local_source
            .streams
            .values()
            .flat_map(|stream| stream.pending.iter().map(|segment| segment.id.clone()))
            .collect::<BTreeSet<_>>();
        self.queue_local_source_segments(
            cluster_id,
            identity,
            signing_key,
            records,
            now_unix_seconds,
        )?;
        let created = self
            .snapshot
            .local_source
            .streams
            .values()
            .flat_map(|stream| stream.pending.iter())
            .filter(|segment| !pending_before.contains(&segment.id))
            .map(|segment| RepositoryReplicaSegment {
                identity: segment.identity.clone(),
                wire: segment.wire.clone(),
            })
            .collect::<Vec<_>>();
        if requires_segment && created.is_empty() {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        Ok(created)
    }

    pub(crate) fn queue_local_history_backfill_batches(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        page_cursor: Option<String>,
        completed: bool,
        batches: Vec<(Vec<SyncRecord>, u64)>,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        for (records, _) in &batches {
            for record in records {
                let stream = if record.is_tombstone() {
                    "tombstone"
                } else {
                    stream_for_schema(record.schema().0).ok_or_else(|| {
                        RepositoryRuntimeError::Storage(format!(
                            "history source schema has no independent stream: {}",
                            record.schema().0
                        ))
                    })?
                };
                if self
                    .snapshot
                    .local_source
                    .streams
                    .get(stream)
                    .is_some_and(|state| !state.pending.is_empty())
                {
                    return Err(RepositoryRuntimeError::StateLimitExceeded);
                }
            }
        }
        let previous_snapshot = self.snapshot.clone();
        let previous_tombstones = self.tombstones.checkpoint();
        let mut created = Vec::new();
        for (records, observed_at) in batches {
            let pending_before = self
                .snapshot
                .local_source
                .streams
                .values()
                .flat_map(|stream| stream.pending.iter().map(|segment| segment.id.clone()))
                .collect::<BTreeSet<_>>();
            if let Err(error) = self.queue_local_source_segments_for_repositories_with_limit(
                cluster_id,
                identity.clone(),
                signing_key,
                records.clone(),
                observed_at,
                LocalSourceQueueOptions {
                    ready_repositories: &["local".to_owned()],
                    _max_pending_segments_per_stream:
                        Self::MAX_HISTORY_BACKFILL_PENDING_SEGMENTS_PER_STREAM,
                    persist: false,
                    defer_journal: true,
                },
            ) {
                self.snapshot = previous_snapshot;
                self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
                return Err(error);
            }
            let new_segments = self
                .snapshot
                .local_source
                .streams
                .values()
                .flat_map(|stream| stream.pending.iter())
                .filter(|segment| !pending_before.contains(&segment.id))
                .map(|segment| RepositoryReplicaSegment {
                    identity: segment.identity.clone(),
                    wire: segment.wire.clone(),
                })
                .collect::<Vec<_>>();
            if !records.is_empty() && new_segments.is_empty() {
                self.snapshot = previous_snapshot;
                self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
            created.extend(new_segments);
        }
        self.snapshot.local_history_backfill_inflight = Some(LocalHistoryBackfillInFlight {
            page_cursor,
            completed,
            segment_ids: created
                .iter()
                .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
                .collect(),
        });
        let persist_result = if self.storage.is_sqlite() {
            let journal_rows = created
                .iter()
                .map(|segment| SourceDeliveryJournalRow {
                    id: hex::encode(Sha256::digest(&segment.wire)),
                    stream: stream_for_wire(&segment.wire),
                    closed_at_unix_seconds: SignedSegment::from_wire(&segment.wire)
                        .map(|signed| signed.canonical().closed_at_unix_seconds())
                        .unwrap_or_default(),
                    identity: segment.identity.clone(),
                    wire: segment.wire.clone(),
                })
                .collect::<Vec<_>>();
            self.persist_control_state_with_journal(&journal_rows)
        } else {
            self.persist_control_state()
        };
        if let Err(error) = persist_result {
            self.snapshot = previous_snapshot;
            self.tombstones = TombstoneLedger::from_checkpoint(previous_tombstones)?;
            return Err(error);
        }
        Ok(created)
    }

    pub(crate) fn queue_local_source_segments_for_repositories(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: Vec<SyncRecord>,
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
        records: Vec<SyncRecord>,
        now_unix_seconds: u64,
        options: LocalSourceQueueOptions<'_>,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        self.hydrate_source_delivery_journal()?;
        let previous_snapshot = self.snapshot.clone();
        let previous_tombstones = self.tombstones.checkpoint();
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
        }
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
            return Ok(self.local_source_pending_segments());
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
                if let Err(error) = self.persist_control_state_with_journal(&journal_rows) {
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
        Ok(self.local_source_pending_segments())
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
                records,
                now_unix_seconds,
            )?
            .into_iter()
            .next())
    }

    pub(crate) fn acknowledge_local_source_segment(
        &mut self,
        delivered_wire: &[u8],
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        if self.remove_local_source_pending_segment(delivered_wire)? {
            if let Err(error) = self.persist_control_state() {
                self.snapshot = previous_snapshot;
                return Err(error);
            }
            if self.storage.is_sqlite() {
                self.storage
                    .acknowledge_source_delivery_journal(&[hex::encode(Sha256::digest(
                        delivered_wire,
                    ))])
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
                self.hydrate_source_delivery_journal()?;
            }
        }
        Ok(())
    }

    /// Persist acknowledgement and the local historical-export cursor together. Replaying an
    /// already accepted segment is safe after a crash; moving only one of these checkpoints is
    /// not, because a new source sequence would otherwise be assigned to the same history row.
    pub(crate) fn acknowledge_local_source_segment_and_checkpoint_backfill(
        &mut self,
        delivered_wire: &[u8],
        page_cursor: Option<String>,
        completed: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        if !self.remove_local_source_pending_segment(delivered_wire)? {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill acknowledgement was not pending".to_owned(),
            ));
        }
        self.snapshot.local_history_backfill_cursor = page_cursor;
        self.snapshot.local_history_backfill_completed = completed;
        self.persist_control_state()?;
        if self.storage.is_sqlite() {
            self.storage
                .acknowledge_source_delivery_journal(&[hex::encode(Sha256::digest(delivered_wire))])
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            self.hydrate_source_delivery_journal()?;
        }
        Ok(())
    }

    pub(crate) fn local_history_backfill_inflight_checkpoint(
        &self,
    ) -> Option<(Option<String>, bool)> {
        self.snapshot
            .local_history_backfill_inflight
            .as_ref()
            .map(|inflight| (inflight.page_cursor.clone(), inflight.completed))
    }

    pub(crate) fn local_history_backfill_inflight_segments(
        &self,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        let Some(inflight) = &self.snapshot.local_history_backfill_inflight else {
            return Ok(Vec::new());
        };
        inflight
            .segment_ids
            .iter()
            .map(|segment_id| {
                self.snapshot
                    .local_source
                    .streams
                    .values()
                    .flat_map(|stream| stream.pending.iter())
                    .find(|segment| &segment.id == segment_id)
                    .map(|segment| RepositoryReplicaSegment {
                        identity: segment.identity.clone(),
                        wire: segment.wire.clone(),
                    })
                    .ok_or_else(|| {
                        RepositoryRuntimeError::Storage(
                            "local history backfill segment is missing".to_owned(),
                        )
                    })
            })
            .collect()
    }

    pub(crate) fn acknowledge_local_source_segments_and_checkpoint_backfill(
        &mut self,
        delivered_segments: &[RepositoryReplicaSegment],
        page_cursor: Option<String>,
        completed: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        let Some(inflight) = self.snapshot.local_history_backfill_inflight.take() else {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill acknowledgement has no inflight page".to_owned(),
            ));
        };
        let delivered_ids = delivered_segments
            .iter()
            .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
            .collect::<Vec<_>>();
        if delivered_ids != inflight.segment_ids {
            self.snapshot = previous_snapshot;
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill acknowledgement page is incomplete".to_owned(),
            ));
        }
        for segment in delivered_segments {
            match self.remove_local_source_pending_segment(&segment.wire) {
                Ok(true) => {}
                Ok(false) => {
                    self.snapshot = previous_snapshot;
                    return Err(RepositoryRuntimeError::Storage(
                        "local history backfill acknowledgement was not pending".to_owned(),
                    ));
                }
                Err(error) => {
                    self.snapshot = previous_snapshot;
                    return Err(error);
                }
            }
        }
        self.snapshot.local_history_backfill_cursor = page_cursor;
        self.snapshot.local_history_backfill_completed = completed;
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        if self.storage.is_sqlite() {
            let ids = delivered_segments
                .iter()
                .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
                .collect::<Vec<_>>();
            self.storage
                .acknowledge_source_delivery_journal(&ids)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            self.hydrate_source_delivery_journal()?;
        }
        Ok(())
    }

    fn remove_local_source_pending_segment(
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
        pending
            .into_iter()
            .map(|(_, pending)| pending)
            .map(|pending| RepositoryReplicaSegment {
                identity: pending.identity.clone(),
                wire: pending.wire.clone(),
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn local_source_next_sequence(&self, stream: &str) -> Option<u64> {
        self.snapshot
            .local_source
            .streams
            .get(stream)
            .map(|state| state.next_sequence)
    }

    pub(crate) fn local_source_backpressure_gaps(
        &self,
        source_node_id: &str,
    ) -> Vec<RepositoryReplicaGap> {
        self.snapshot
            .local_source
            .backpressure_gaps
            .iter()
            .map(|(key, gap)| RepositoryReplicaGap {
                source_node_id: source_node_id.to_owned(),
                source_epoch: gap.source_epoch,
                stream: backpressure_gap_stream(key).to_owned(),
                first_sequence: gap.first_sequence,
                last_sequence: gap.last_sequence,
                start_unix_seconds: gap.start_unix_seconds,
                end_unix_seconds: gap.end_unix_seconds,
                permanent: false,
                reason: None,
            })
            .collect()
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
                ))
                .is_some_and(|key| self.tombstones.fully_acknowledged(key))
        }))
    }

    pub(crate) fn complete_local_source_tombstones(
        &mut self,
        markers: &[crate::node_history::RepositoryHistoryDeletionMarker],
    ) -> Result<(), RepositoryRuntimeError> {
        let mut changed = false;
        for marker in markers {
            changed |= self
                .snapshot
                .local_source
                .deletion_marker_keys
                .remove(&deletion_marker_id_parts(
                    &marker.schema_id,
                    &marker.record_key,
                ))
                .is_some();
        }
        if changed {
            self.persist_control_state()?;
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
    deletion_marker_id_parts(schema_id, record.record_key())
}

fn deletion_marker_id_parts(schema_id: &str, record_key: &[u8]) -> String {
    format!("{schema_id}:{}", hex::encode(record_key))
}

pub(crate) fn source_epoch(cluster_id: &str, node_id: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(b"xp-history-source-epoch-v1\0");
    hasher.update(cluster_id.as_bytes());
    hasher.update([0]);
    hasher.update(node_id.as_bytes());
    hasher.update(b"stable-source-epoch");
    let bytes: [u8; 32] = hasher.finalize().into();
    (u64::from_be_bytes(bytes[..8].try_into().expect("SHA-256 prefix")) & i64::MAX as u64).max(1)
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
