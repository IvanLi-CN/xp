use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor as IoCursor, Read as _},
};

use serde::{Deserialize, Serialize};

use crate::history_sync::{
    MAX_DECOMPRESSION_EXPANSION_RATIO, MAX_RELAY_PLAINTEXT_BYTES, MAX_RESPONSE_WIRE_BYTES,
    SignedSegment, SyncRecord,
};

use super::{
    RelaySegmentCursor, RepositoryReplicaRuntime, RepositoryRuntimeError,
    RepositoryTombstoneAcknowledgement, StoredGap,
};
use crate::state::history_repository::replica::{
    AntiEntropySchedule, CollectorSelector, ReplicaError, ReplicaRecordKey, ReplicaWork,
    rendezvous_collectors,
};

const MAX_REPAIR_SEGMENTS: usize = 64;
const MAX_REPAIR_GAPS: usize = 64;
const MAX_RELAY_BATCH_DECODED_BYTES: usize = 1024 * 1024;
const MAX_RELAY_TARGETS: usize = 64;
const MAX_COLLECTION_SOURCES: usize = 4_096;
const MAX_TOMBSTONE_ACKNOWLEDGEMENTS_PER_CYCLE: usize = 64;
const MAX_RETAINED_PARTITION_SUMMARIES: usize = 1_024;
const COLLECTION_STALE_SECONDS: u64 = 5 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryReplicaSummary {
    pub(crate) segment_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) next_segment_id: Option<String>,
    /// Daily verification is over signed source-stream ranges, not opaque segment ids.
    pub(crate) partitions: Vec<RepositoryPartitionSummary>,
    #[serde(default)]
    pub(crate) partitions_included: bool,
    pub(crate) gaps: Vec<RepositoryReplicaGap>,
    pub(crate) last_verified_unix_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RepositoryPartitionSummary {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    partition: u32,
    first_sequence: u64,
    last_sequence: u64,
    hash: [u8; 32],
    record_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RepositoryReplicaGap {
    pub(crate) source_node_id: String,
    pub(crate) source_epoch: u64,
    pub(crate) stream: String,
    pub(crate) first_sequence: u64,
    pub(crate) last_sequence: u64,
    pub(crate) start_unix_seconds: u64,
    pub(crate) end_unix_seconds: u64,
    pub(crate) permanent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryReplicaSegment {
    pub(crate) identity: crate::state::history_repository::identity::RepositoryNodeIdentity,
    pub(crate) wire: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryRepairBatch {
    pub(crate) segments: Vec<RepositoryReplicaSegment>,
    #[serde(default)]
    pub(crate) gaps: Vec<RepositoryReplicaGap>,
}

pub(crate) struct RelayRepairPayload {
    pub(crate) batch: RepositoryRepairBatch,
    pub(crate) bytes: Vec<u8>,
}

impl RepositoryRepairBatch {
    pub(crate) fn frame_sized_relay_payload(
        self,
    ) -> Result<RelayRepairPayload, RepositoryRuntimeError> {
        if self.segments.len() > MAX_REPAIR_SEGMENTS || self.gaps.len() > MAX_REPAIR_GAPS {
            return Err(ReplicaError::RepairLimitExceeded.into());
        }
        validate_replica_gaps(&self.gaps)?;

        let gaps = self.gaps;
        let mut selected = Vec::new();
        let mut bytes = encode_relay_repair_batch(&RepositoryRepairBatch {
            segments: Vec::new(),
            gaps: gaps.clone(),
        })?;
        if bytes.len() > MAX_RELAY_PLAINTEXT_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }

        let mut selected_wire_bytes = 0usize;
        for segment in self.segments {
            let next_wire_bytes = selected_wire_bytes.saturating_add(segment.wire.len());
            if !selected.is_empty() && next_wire_bytes > MAX_RELAY_PLAINTEXT_BYTES {
                break;
            }
            let mut candidate = selected.clone();
            candidate.push(segment);
            let candidate_batch = RepositoryRepairBatch {
                segments: candidate,
                gaps: gaps.clone(),
            };
            let candidate_bytes = encode_relay_repair_batch(&candidate_batch)?;
            if candidate_bytes.len() > MAX_RELAY_PLAINTEXT_BYTES {
                if selected.is_empty() {
                    return Err(RepositoryRuntimeError::StateLimitExceeded);
                }
                break;
            }
            selected = candidate_batch.segments;
            selected_wire_bytes = next_wire_bytes;
            bytes = candidate_bytes;
        }

        Ok(RelayRepairPayload {
            batch: RepositoryRepairBatch {
                segments: selected,
                gaps,
            },
            bytes,
        })
    }

    pub(crate) fn from_relay_payload(payload: &[u8]) -> Result<Self, RepositoryRuntimeError> {
        if payload.len() > MAX_RELAY_PLAINTEXT_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let mut decoder =
            zstd::stream::read::Decoder::new(IoCursor::new(payload)).map_err(|_| {
                RepositoryRuntimeError::Storage("relay payload is malformed".to_owned())
            })?;
        let mut decoded = Vec::with_capacity(payload.len());
        let mut chunk = [0_u8; 8 * 1024];
        let max_expanded_len = payload
            .len()
            .saturating_mul(MAX_DECOMPRESSION_EXPANSION_RATIO)
            .min(MAX_RELAY_BATCH_DECODED_BYTES);
        loop {
            let read = decoder.read(&mut chunk).map_err(|_| {
                RepositoryRuntimeError::Storage("relay payload is malformed".to_owned())
            })?;
            if read == 0 {
                break;
            }
            if decoded.len().saturating_add(read) > max_expanded_len {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
            decoded.extend_from_slice(&chunk[..read]);
        }
        let batch = serde_json::from_slice::<Self>(&decoded).map_err(|_| {
            RepositoryRuntimeError::Storage("relay payload is malformed".to_owned())
        })?;
        if batch.segments.len() > MAX_REPAIR_SEGMENTS || batch.gaps.len() > MAX_REPAIR_GAPS {
            return Err(ReplicaError::RepairLimitExceeded.into());
        }
        validate_replica_gaps(&batch.gaps)?;
        Ok(batch)
    }
}

#[derive(Debug)]
pub(crate) struct RelayRepairPage {
    pub(crate) batch: RepositoryRepairBatch,
    pub(crate) payload: Vec<u8>,
    next_segment_id: Option<String>,
}

impl RelayRepairPage {
    pub(crate) fn next_segment_id(&self) -> Option<&str> {
        self.next_segment_id.as_deref()
    }
}

#[derive(Debug)]
pub(crate) struct TombstoneAcknowledgementPage {
    acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
    next_cursor: Option<ReplicaRecordKey>,
}

impl TombstoneAcknowledgementPage {
    pub(crate) fn acknowledgements(&self) -> &[RepositoryTombstoneAcknowledgement] {
        &self.acknowledgements
    }

    pub(crate) fn next_cursor(&self) -> Option<&ReplicaRecordKey> {
        self.next_cursor.as_ref()
    }
}

impl RepositoryReplicaRuntime {
    pub(crate) fn prepare_for_replication(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        self.rebuild_if_stale(now_unix_seconds)?;
        self.prune_retention(now_unix_seconds)
    }

    pub(crate) fn replication_work(&self, now_unix_seconds: u64) -> ReplicaWork {
        AntiEntropySchedule::default().due(
            now_unix_seconds,
            self.snapshot.last_deep_verification_unix_seconds,
            self.snapshot.last_anti_entropy_unix_seconds,
        )
    }

    pub(crate) fn record_replication_completed(
        &mut self,
        now_unix_seconds: u64,
        work: ReplicaWork,
    ) -> Result<(), RepositoryRuntimeError> {
        if !work.is_anti_entropy() {
            return Ok(());
        }
        self.snapshot.last_anti_entropy_unix_seconds = Some(now_unix_seconds);
        if work.is_deep_verification() {
            self.snapshot.last_deep_verification_unix_seconds = Some(now_unix_seconds);
            self.snapshot.deep_verified_peer_ids.clear();
        }
        self.persist_control_state()
    }

    pub(crate) fn reconcile_ready_repositories(
        &mut self,
        ready_repositories: &[String],
    ) -> Result<(), RepositoryRuntimeError> {
        self.tombstones
            .reconcile_ready_repositories(ready_repositories)?;
        self.snapshot
            .relay_segment_cursors
            .retain(|repository_id, _| ready_repositories.contains(repository_id));
        self.persist_control_state()
    }

    pub(crate) fn tombstone_acknowledgement_page(
        &self,
        local_repository_id: &str,
    ) -> Result<TombstoneAcknowledgementPage, RepositoryRuntimeError> {
        let keys = self.tombstones.acknowledgement_keys_for(
            local_repository_id,
            self.snapshot.tombstone_acknowledgement_cursor.as_ref(),
            MAX_TOMBSTONE_ACKNOWLEDGEMENTS_PER_CYCLE,
        )?;
        let next_cursor = keys.last().cloned();
        Ok(TombstoneAcknowledgementPage {
            acknowledgements: keys
                .into_iter()
                .map(|key| RepositoryTombstoneAcknowledgement {
                    key,
                    repository_id: local_repository_id.to_owned(),
                })
                .collect(),
            next_cursor,
        })
    }

    pub(crate) fn record_tombstone_acknowledgement_delivery(
        &mut self,
        next_cursor: Option<&ReplicaRecordKey>,
    ) -> Result<(), RepositoryRuntimeError> {
        let previous_snapshot = self.snapshot.clone();
        self.snapshot.tombstone_acknowledgement_cursor = next_cursor.cloned();
        if let Err(error) = self.persist_control_state() {
            self.snapshot = previous_snapshot;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn next_replication_peers(
        &mut self,
        ready_repositories: &[String],
        local_repository_id: &str,
        max_peers: usize,
    ) -> Result<Vec<String>, RepositoryRuntimeError> {
        if max_peers == 0 {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let mut peers = ready_repositories
            .iter()
            .filter(|repository_id| repository_id.as_str() != local_repository_id)
            .cloned()
            .collect::<Vec<_>>();
        peers.sort_unstable();
        peers.dedup();
        if peers.is_empty() {
            return Ok(Vec::new());
        }
        let start = self.snapshot.replication_peer_offset % peers.len();
        let count = max_peers.min(peers.len());
        let selected = (0..count)
            .map(|offset| peers[(start + offset) % peers.len()].clone())
            .collect::<Vec<_>>();
        self.snapshot.replication_peer_offset = (start + count) % peers.len();
        self.snapshot
            .deep_verified_peer_ids
            .retain(|repository_id| peers.binary_search(repository_id).is_ok());
        self.persist_control_state()?;
        Ok(selected)
    }

    pub(crate) fn record_direct_peer_deep_verification(
        &mut self,
        peer_repository_id: &str,
        ready_repositories: &[String],
        local_repository_id: &str,
        work: ReplicaWork,
    ) -> Result<bool, RepositoryRuntimeError> {
        if !work.is_deep_verification() {
            return Ok(false);
        }
        let required = ready_repositories
            .iter()
            .filter(|repository_id| repository_id.as_str() != local_repository_id)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !required.contains(peer_repository_id) {
            return Err(RepositoryRuntimeError::Replica(
                ReplicaError::InvalidIdentifier,
            ));
        }
        self.snapshot
            .deep_verified_peer_ids
            .retain(|repository_id| required.contains(repository_id));
        self.snapshot
            .deep_verified_peer_ids
            .insert(peer_repository_id.to_owned());
        let complete = required.is_subset(&self.snapshot.deep_verified_peer_ids);
        self.persist_control_state()?;
        Ok(complete)
    }

    pub(crate) fn collects_source(
        &self,
        source_node_id: &str,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<bool, RepositoryRuntimeError> {
        let assignment = rendezvous_collectors(source_node_id, ready_repositories)?;
        let selector =
            CollectorSelector::from_failure_cycles(self.snapshot.collector_failure_cycles.clone());
        Ok(selector.select(source_node_id, &assignment)? == local_repository_id)
    }

    pub(crate) fn accepts_source(
        &self,
        source_node_id: &str,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<bool, RepositoryRuntimeError> {
        let assignment = rendezvous_collectors(source_node_id, ready_repositories)?;
        Ok(assignment.primary() == local_repository_id
            || assignment.standby() == Some(local_repository_id))
    }

    pub(crate) fn record_collection_cycle(
        &mut self,
        source_node_id: &str,
        ready_repositories: &[String],
        local_repository_id: &str,
        succeeded: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        let assignment = rendezvous_collectors(source_node_id, ready_repositories)?;
        if assignment.primary() != local_repository_id {
            return Ok(());
        }
        let mut selector =
            CollectorSelector::from_failure_cycles(self.snapshot.collector_failure_cycles.clone());
        selector.record_primary_cycle(source_node_id, succeeded)?;
        self.snapshot.collector_failure_cycles = selector.failure_cycles().clone();
        self.persist_control_state()
    }

    pub(crate) fn record_stale_collection_cycles(
        &mut self,
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
        known_source_node_ids: &[String],
    ) -> Result<(), RepositoryRuntimeError> {
        let mut sources = self
            .snapshot
            .source_last_received_unix_seconds
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        sources.extend(known_source_node_ids.iter().cloned());
        let stale_sources = sources
            .into_iter()
            .take(MAX_COLLECTION_SOURCES)
            .filter(|source| {
                self.snapshot
                    .source_last_received_unix_seconds
                    .get(source)
                    .is_none_or(|last_received| {
                        now_unix_seconds.saturating_sub(*last_received) >= COLLECTION_STALE_SECONDS
                    })
            })
            .collect::<Vec<_>>();
        for source_node_id in stale_sources {
            self.record_collection_cycle(
                &source_node_id,
                ready_repositories,
                local_repository_id,
                false,
            )?;
        }
        Ok(())
    }

    pub(crate) fn replication_summary(
        &self,
    ) -> Result<RepositoryReplicaSummary, RepositoryRuntimeError> {
        self.replication_summary_after(None, true)
    }

    pub(crate) fn replication_summary_after(
        &self,
        after_segment_id: Option<&str>,
        deep_verification: bool,
    ) -> Result<RepositoryReplicaSummary, RepositoryRuntimeError> {
        self.require_legacy_segment_cursor_index()?;
        let mut segments = self.stored_segments_page(
            after_segment_id,
            super::REPLICATION_SEGMENT_PAGE_SIZE.saturating_add(1),
        )?;
        let has_next_page = segments.len() > super::REPLICATION_SEGMENT_PAGE_SIZE;
        segments.truncate(super::REPLICATION_SEGMENT_PAGE_SIZE);
        let next_segment_id = if has_next_page {
            Some(segment_sync_cursor(
                segments.last().expect("nonempty page had an extra segment"),
            ))
        } else {
            None
        };
        Ok(RepositoryReplicaSummary {
            segment_ids: segments.iter().map(|segment| segment.id.clone()).collect(),
            partitions: if deep_verification && after_segment_id.is_none() {
                self.retained_partition_summaries()?
            } else {
                Vec::new()
            },
            partitions_included: deep_verification && after_segment_id.is_none(),
            gaps: self.snapshot.gaps.iter().map(gap_summary).collect(),
            last_verified_unix_seconds: self.snapshot.last_verified_unix_seconds,
            next_segment_id,
        })
    }

    pub(crate) fn missing_segment_ids(
        &self,
        remote: &RepositoryReplicaSummary,
        _deep_verification: bool,
    ) -> Result<Vec<String>, RepositoryRuntimeError> {
        let local_segments = self.stored_segments_by_ids(&remote.segment_ids)?;
        let local_ids = local_segments
            .iter()
            .map(|segment| segment.id.as_str())
            .collect::<BTreeSet<_>>();
        Ok(remote
            .segment_ids
            .iter()
            .filter(|id| !local_ids.contains(id.as_str()))
            .take(MAX_REPAIR_SEGMENTS)
            .cloned()
            .collect())
    }

    pub(crate) fn requires_repair(
        &self,
        remote: &RepositoryReplicaSummary,
        deep_verification: bool,
    ) -> Result<bool, RepositoryRuntimeError> {
        let gaps_converged = canonical_gaps(self.snapshot.gaps.iter().map(gap_summary))
            == canonical_gaps(remote.gaps.iter().cloned());
        let partitions_converged = !deep_verification
            || !remote.partitions_included
            || self.retained_partition_summaries()? == remote.partitions;
        // The remote summary is keyset-paged. Its page is complete only when every advertised
        // segment is present locally; peer-owned extra pages converge on the peer's next cycle.
        let segments_converged = self.missing_segment_ids(remote, false)?.is_empty();
        Ok(!(segments_converged && gaps_converged && partitions_converged))
    }

    pub(crate) fn retained_partitions_converged(
        &self,
        remote: &RepositoryReplicaSummary,
    ) -> Result<bool, RepositoryRuntimeError> {
        if !remote.partitions_included {
            return Ok(true);
        }
        Ok(self.retained_partition_summaries()? == remote.partitions)
    }

    pub(crate) fn repair_batch(
        &self,
        requested_segment_ids: &[String],
    ) -> Result<RepositoryRepairBatch, RepositoryRuntimeError> {
        self.require_legacy_segment_cursor_index()?;
        if requested_segment_ids.len() > MAX_REPAIR_SEGMENTS {
            return Err(RepositoryRuntimeError::Replica(
                ReplicaError::RepairLimitExceeded,
            ));
        }
        let requested = requested_segment_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut stored_segments = self
            .stored_segments_by_ids(requested_segment_ids)?
            .into_iter()
            .map(|segment| Ok((repair_segment_order(&segment)?, segment)))
            .collect::<Result<Vec<_>, RepositoryRuntimeError>>()?;
        stored_segments.sort_by(|(left, _), (right, _)| left.cmp(right));
        let mut response_bytes = 0usize;
        let mut segments = Vec::new();
        for (_, segment) in stored_segments {
            if !requested.contains(segment.id.as_str()) {
                continue;
            }
            let next = segment.wire.len().saturating_add(128);
            if response_bytes.saturating_add(next) > MAX_RESPONSE_WIRE_BYTES {
                break;
            }
            response_bytes = response_bytes.saturating_add(next);
            segments.push(RepositoryReplicaSegment {
                identity: segment.identity,
                wire: segment.wire,
            });
        }
        Ok(RepositoryRepairBatch {
            segments,
            gaps: canonical_gaps(self.snapshot.gaps.iter().map(gap_summary)),
        })
    }

    pub(crate) fn relay_batch(
        &self,
        target_repository_id: &str,
    ) -> Result<RelayRepairPage, RepositoryRuntimeError> {
        self.require_legacy_segment_cursor_index()?;
        let after_id = match self
            .snapshot
            .relay_segment_cursors
            .get(target_repository_id)
        {
            Some(RelaySegmentCursor::NextSegmentId(next_segment_id)) => {
                Some(next_segment_id.as_str())
            }
            _ => None,
        };
        let mut candidates =
            self.stored_segments_page(after_id, MAX_REPAIR_SEGMENTS.saturating_add(1))?;
        if candidates.is_empty() && after_id.is_some() {
            candidates = self.stored_segments_page(None, MAX_REPAIR_SEGMENTS.saturating_add(1))?;
        }
        candidates.truncate(MAX_REPAIR_SEGMENTS);
        let candidate_ids = candidates
            .iter()
            .map(segment_sync_cursor)
            .collect::<Vec<_>>();
        let payload = RepositoryRepairBatch {
            segments: candidates
                .into_iter()
                .map(|segment| RepositoryReplicaSegment {
                    identity: segment.identity,
                    wire: segment.wire,
                })
                .collect(),
            gaps: canonical_gaps(self.snapshot.gaps.iter().map(gap_summary)),
        }
        .frame_sized_relay_payload()?;
        let next_segment_id = payload
            .batch
            .segments
            .len()
            .checked_sub(1)
            .and_then(|index| candidate_ids.get(index).cloned());
        Ok(RelayRepairPage {
            batch: payload.batch,
            payload: payload.bytes,
            next_segment_id,
        })
    }

    pub(crate) fn record_relay_batch_delivered(
        &mut self,
        target_repository_id: &str,
        next_segment_id: Option<&str>,
    ) -> Result<(), RepositoryRuntimeError> {
        let Some(next_segment_id) = next_segment_id else {
            self.snapshot
                .relay_segment_cursors
                .remove(target_repository_id);
            return self.persist_control_state();
        };
        if !self
            .snapshot
            .relay_segment_cursors
            .contains_key(target_repository_id)
            && self.snapshot.relay_segment_cursors.len() == MAX_RELAY_TARGETS
        {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        self.snapshot.relay_segment_cursors.insert(
            target_repository_id.to_owned(),
            RelaySegmentCursor::NextSegmentId(next_segment_id.to_owned()),
        );
        self.persist_control_state()
    }

    pub(crate) fn merge_replica_gaps(
        &mut self,
        remote_gaps: &[RepositoryReplicaGap],
    ) -> Result<(), RepositoryRuntimeError> {
        validate_replica_gaps(remote_gaps)?;
        let mut merged = canonical_gaps(self.snapshot.gaps.iter().map(gap_summary));
        merged.extend(canonical_gaps(remote_gaps.iter().cloned()));
        let mut merged = canonical_gaps(merged);
        if merged.len() > MAX_REPAIR_GAPS {
            self.snapshot.history_truncated = true;
            merged.truncate(MAX_REPAIR_GAPS);
        }
        self.snapshot.gaps = merged.into_iter().map(stored_gap).collect();
        self.persist_control_state()
    }

    pub(crate) fn acknowledge_tombstones(
        &mut self,
        acknowledgements: &[RepositoryTombstoneAcknowledgement],
    ) -> Result<(), RepositoryRuntimeError> {
        for acknowledgement in acknowledgements {
            match self
                .tombstones
                .acknowledge(acknowledgement.key.clone(), &acknowledgement.repository_id)
            {
                Ok(()) | Err(ReplicaError::TombstoneMissing) => {}
                Err(error) => return Err(error.into()),
            }
        }
        self.persist_control_state()
    }

    pub(super) fn persist_control_state(&mut self) -> Result<(), RepositoryRuntimeError> {
        self.snapshot.tombstones = self.tombstones.checkpoint();
        let bytes = serde_json::to_vec(&self.snapshot)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if bytes.len() > super::MAX_RUNTIME_STATE_BYTES {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let result = self.storage.write(
            crate::state::history_storage::REPOSITORY_REPLICA_KEY,
            &bytes,
        );
        if let Err(error) = result {
            if self.uses_sqlite_history() {
                self.storage_degraded = true;
            }
            return Err(RepositoryRuntimeError::Storage(error.to_string()));
        }
        Ok(())
    }

    fn retained_partition_summaries(
        &self,
    ) -> Result<Vec<RepositoryPartitionSummary>, RepositoryRuntimeError> {
        let mut summaries = BTreeMap::new();
        if self.uses_sqlite_history() {
            let mut offset = 0;
            loop {
                let page = self.sqlite_records(None, None, None, offset, 1_000)?;
                let page_len = page.len();
                accumulate_record_partitions(&mut summaries, page.iter())?;
                if page_len < 1_000 {
                    break;
                }
                offset += page_len;
            }
        } else {
            let mut records = self
                .snapshot
                .records
                .iter()
                .filter(|record| !record.tombstone)
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| {
                (
                    record.observed_at_unix_seconds,
                    record.source_node_id.clone(),
                    record.source_epoch,
                    record.stream.clone(),
                    record.sequence,
                )
            });
            accumulate_record_partitions(&mut summaries, records.iter())?;
        }
        Ok(summaries.into_values().collect())
    }
}

fn accumulate_record_partitions<'a>(
    summaries: &mut BTreeMap<(String, u64, String, u32), RepositoryPartitionSummary>,
    records: impl IntoIterator<Item = &'a super::StoredRecord>,
) -> Result<(), RepositoryRuntimeError> {
    for record in records {
        let record_summary = record_partition_summary(record)?;
        let mut key = (
            record_summary.source_node_id.clone(),
            record_summary.source_epoch,
            record_summary.stream.clone(),
            record_summary.partition,
        );
        if !summaries.contains_key(&key)
            && summaries.len() >= MAX_RETAINED_PARTITION_SUMMARIES.saturating_sub(1)
        {
            key = (String::new(), u64::MAX, String::new(), u32::MAX);
        }
        if let Some(summary) = summaries.get_mut(&key) {
            summary.first_sequence = summary.first_sequence.min(record_summary.first_sequence);
            summary.last_sequence = summary.last_sequence.max(record_summary.last_sequence);
            summary.hash = combine_partition_hash(summary.hash, record_summary.hash);
            summary.record_count = summary.record_count.saturating_add(1);
        } else {
            let summary = if key.0.is_empty() {
                RepositoryPartitionSummary {
                    source_node_id: String::new(),
                    source_epoch: u64::MAX,
                    stream: String::new(),
                    partition: u32::MAX,
                    first_sequence: record_summary.first_sequence,
                    last_sequence: record_summary.last_sequence,
                    hash: record_summary.hash,
                    record_count: 1,
                }
            } else {
                record_summary
            };
            summaries.insert(key, summary);
        }
    }
    Ok(())
}

fn repair_segment_order(
    segment: &super::StoredSegment,
) -> Result<(bool, String, u64, String, u64), RepositoryRuntimeError> {
    let signed = SignedSegment::from_wire(&segment.wire)?;
    let first_cursor = signed.canonical().first_cursor();
    Ok((
        !signed
            .canonical()
            .records()
            .iter()
            .any(SyncRecord::is_tombstone),
        first_cursor.source_node_id().to_owned(),
        first_cursor.source_epoch(),
        first_cursor.stream().to_owned(),
        first_cursor.sequence(),
    ))
}

fn segment_sync_cursor(segment: &super::StoredSegment) -> String {
    let phase = if super::storage::segment_tombstone_rank(segment) {
        'r'
    } else {
        't'
    };
    format!("{phase}:{}", segment.id)
}

fn record_partition_summary(
    record: &super::StoredRecord,
) -> Result<RepositoryPartitionSummary, RepositoryRuntimeError> {
    use sha2::Digest as _;

    let mut hasher = sha2::Sha256::new();
    hasher.update(b"xp-history-repository-record-v1\0");
    hasher.update(record.source_node_id.as_bytes());
    hasher.update(record.source_epoch.to_be_bytes());
    hasher.update(record.stream.as_bytes());
    hasher.update(record.sequence.to_be_bytes());
    hasher.update(record.subject_node_id.as_bytes());
    hasher.update(record.observer_node_id.as_bytes());
    hasher.update(record.schema_id.as_bytes());
    hasher.update(record.schema_version.to_be_bytes());
    hasher.update(&record.record_key);
    hasher.update(record.observed_at_unix_seconds.to_be_bytes());
    hasher.update(&record.payload);
    Ok(RepositoryPartitionSummary {
        source_node_id: record.source_node_id.clone(),
        source_epoch: record.source_epoch,
        stream: record.stream.clone(),
        partition: u32::try_from(record.observed_at_unix_seconds / (365 * 24 * 60 * 60))
            .map_err(|_| RepositoryRuntimeError::Replica(ReplicaError::InvalidRange))?,
        first_sequence: record.sequence,
        last_sequence: record.sequence,
        hash: hasher.finalize().into(),
        record_count: 1,
    })
}

fn combine_partition_hash(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    use sha2::Digest as _;

    let mut hasher = sha2::Sha256::new();
    hasher.update(b"xp-history-repository-partition-v1\0");
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

fn gap_summary(gap: &StoredGap) -> RepositoryReplicaGap {
    RepositoryReplicaGap {
        source_node_id: gap.source_node_id.clone(),
        source_epoch: gap.source_epoch,
        stream: gap.stream.clone(),
        first_sequence: gap.first_sequence,
        last_sequence: gap.last_sequence,
        start_unix_seconds: gap.start_unix_seconds,
        end_unix_seconds: gap.end_unix_seconds,
        permanent: gap.permanent,
    }
}

fn stored_gap(gap: RepositoryReplicaGap) -> StoredGap {
    StoredGap {
        source_node_id: gap.source_node_id,
        source_epoch: gap.source_epoch,
        stream: gap.stream,
        first_sequence: gap.first_sequence,
        last_sequence: gap.last_sequence,
        start_unix_seconds: gap.start_unix_seconds,
        end_unix_seconds: gap.end_unix_seconds,
        permanent: gap.permanent,
    }
}

fn encode_relay_repair_batch(
    batch: &RepositoryRepairBatch,
) -> Result<Vec<u8>, RepositoryRuntimeError> {
    let serialized = serde_json::to_vec(batch)
        .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
    zstd::stream::encode_all(IoCursor::new(serialized), 1)
        .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
}

fn validate_replica_gaps(gaps: &[RepositoryReplicaGap]) -> Result<(), RepositoryRuntimeError> {
    if gaps.len() > MAX_REPAIR_GAPS {
        return Err(ReplicaError::RepairLimitExceeded.into());
    }
    if gaps.iter().any(|gap| {
        gap.source_node_id.is_empty()
            || gap.stream.is_empty()
            || gap.first_sequence > gap.last_sequence
            || gap.start_unix_seconds > gap.end_unix_seconds
    }) {
        return Err(ReplicaError::InvalidRange.into());
    }
    Ok(())
}

fn canonical_gaps(
    gaps: impl IntoIterator<Item = RepositoryReplicaGap>,
) -> Vec<RepositoryReplicaGap> {
    let mut gaps = gaps.into_iter().collect::<Vec<_>>();
    gaps.sort_by_key(|gap| {
        (
            gap.source_node_id.clone(),
            gap.source_epoch,
            gap.stream.clone(),
            gap.first_sequence,
            gap.last_sequence,
            gap.permanent,
        )
    });
    gaps.dedup();
    gaps
}
