use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::state::history_repository::replica::RepositoryReplicaGap;

use super::*;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    primary_failure_repository_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    backpressure_gaps: BTreeMap<String, LocalSourceGap>,
    /// Durable marker-to-cursor mapping. Tombstones use their own stream, so their sequence
    /// cannot be reconstructed from the affected schema's live cursor.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    deletion_marker_keys: BTreeMap<String, ReplicaRecordKey>,
}

impl LocalSourceState {
    pub(super) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(super) fn node_id(&self) -> Option<&str> {
        (!self.node_id.is_empty()).then_some(self.node_id.as_str())
    }

    pub(super) fn rotate_after_repository_rebuild(&mut self) {
        if self.epoch != 0 {
            self.epoch = self.epoch.saturating_add(1);
        }
        self.streams.clear();
        self.backpressure_gaps.clear();
        self.deletion_marker_keys.clear();
        self.primary_failure_cycles = 0;
        self.primary_failure_repository_id = None;
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

    pub(crate) fn queue_local_source_segments_for_repositories(
        &mut self,
        cluster_id: &str,
        identity: RepositoryNodeIdentity,
        signing_key: &ed25519_dalek::SigningKey,
        records: Vec<SyncRecord>,
        now_unix_seconds: u64,
        ready_repositories: &[String],
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
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
        // Five streams at the protocol's 256 KiB segment ceiling stay below the 16 MiB state guard.
        const MAX_PENDING_SEGMENTS_PER_STREAM: usize = 8;
        if records_by_stream.is_empty() {
            return Ok(self.local_source_pending_segments());
        }
        if self.snapshot.local_source.epoch == 0 {
            self.snapshot.local_source.epoch = self
                .storage
                .allocate_repository_source_epoch(
                    cluster_id,
                    identity.node_id().as_str(),
                    source_epoch(cluster_id, identity.node_id().as_str()),
                )
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            self.snapshot.local_source.node_id = identity.node_id().as_str().to_owned();
        }
        for (stream, records) in records_by_stream {
            let record_count = u64::try_from(records.len())
                .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?;
            let (pending_segments, backpressured_records, sequence) = {
                let stream_state = self
                    .snapshot
                    .local_source
                    .streams
                    .entry(stream.to_owned())
                    .or_default();
                if stream_state.pending.len() >= MAX_PENDING_SEGMENTS_PER_STREAM {
                    let first_sequence = stream_state.next_sequence;
                    stream_state.next_sequence = stream_state
                        .next_sequence
                        .checked_add(record_count)
                        .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
                    stream_state.backpressured_records = stream_state
                        .backpressured_records
                        .saturating_add(record_count);
                    (
                        Some(stream_state.pending.len()),
                        stream_state.backpressured_records,
                        first_sequence,
                    )
                } else {
                    (None, 0, 0)
                }
            };
            if let Some(pending_segments) = pending_segments {
                self.record_local_source_backpressure_gap(
                    stream,
                    sequence,
                    sequence.saturating_add(record_count.saturating_sub(1)),
                    now_unix_seconds,
                );
                tracing::warn!(
                    stream,
                    pending_segments,
                    backpressured_records,
                    "history source outbox is backpressured"
                );
                continue;
            }
            let next_sequence = self
                .snapshot
                .local_source
                .streams
                .get(stream)
                .expect("source stream was initialized")
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
                self.tombstones
                    .tombstone(key.clone(), now_unix_seconds, ready_repositories)?;
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
            stream_state.pending.push_back(StoredSegment {
                id: hex::encode(Sha256::digest(&wire)),
                closed_at_unix_seconds: now_unix_seconds,
                identity: identity.clone(),
                wire: wire.clone(),
            });
        }
        self.persist_control_state()?;
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
        if self.remove_local_source_pending_segment(delivered_wire) {
            self.persist_control_state()?;
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
        if !self.remove_local_source_pending_segment(delivered_wire) {
            return Err(RepositoryRuntimeError::Storage(
                "local history backfill acknowledgement was not pending".to_owned(),
            ));
        }
        self.snapshot.local_history_backfill_cursor = page_cursor;
        self.snapshot.local_history_backfill_completed = completed;
        self.persist_control_state()
    }

    fn remove_local_source_pending_segment(&mut self, delivered_wire: &[u8]) -> bool {
        let mut acknowledged = false;
        for stream in self.snapshot.local_source.streams.values_mut() {
            if stream
                .pending
                .front()
                .is_some_and(|pending| pending.wire == delivered_wire)
            {
                stream.pending.pop_front();
                acknowledged = true;
            }
        }
        acknowledged
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
            .map(|(stream, gap)| RepositoryReplicaGap {
                source_node_id: source_node_id.to_owned(),
                source_epoch: gap.source_epoch,
                stream: stream.clone(),
                first_sequence: gap.first_sequence,
                last_sequence: gap.last_sequence,
                start_unix_seconds: gap.start_unix_seconds,
                end_unix_seconds: gap.end_unix_seconds,
                permanent: true,
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
        }
        if selected_repository_id != primary_repository_id {
            if succeeded {
                self.snapshot.local_source.primary_failure_cycles = self
                    .snapshot
                    .local_source
                    .primary_failure_cycles
                    .saturating_sub(1);
            }
            return self.persist_control_state();
        }
        if succeeded {
            self.snapshot.local_source.primary_failure_cycles = 0;
        } else {
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
        let gap = self
            .snapshot
            .local_source
            .backpressure_gaps
            .entry(stream.to_owned())
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
mod tests {
    use std::collections::BTreeMap;

    use super::{LocalSourceState, LocalSourceStreamState};

    #[test]
    fn stale_repository_rebuild_rotates_the_durable_source_epoch_before_resetting_sequences() {
        let mut state = LocalSourceState {
            epoch: 7,
            streams: BTreeMap::from([("runtime".to_owned(), LocalSourceStreamState::default())]),
            ..LocalSourceState::default()
        };
        state.rotate_after_repository_rebuild();
        assert_eq!(state.epoch, 8);
        assert!(state.streams.is_empty());
    }
}
