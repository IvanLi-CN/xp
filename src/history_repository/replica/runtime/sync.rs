use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::history_sync::MAX_RESPONSE_WIRE_BYTES;

use super::{
    RepositoryReplicaRuntime, RepositoryRuntimeError, RepositoryTombstoneAcknowledgement, StoredGap,
};
use crate::state::history_repository::replica::{AntiEntropySchedule, ReplicaError, ReplicaWork};

const MAX_REPAIR_SEGMENTS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryReplicaSummary {
    pub(crate) segment_ids: Vec<String>,
    /// Daily verification is over signed source-stream ranges, not opaque segment ids.
    pub(crate) partitions: Vec<RepositoryPartitionSummary>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl RepositoryReplicaRuntime {
    pub(crate) fn prepare_for_replication(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        self.rebuild_if_stale(now_unix_seconds)
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
        }
        self.persist_control_state()
    }

    pub(crate) fn reconcile_ready_repositories(
        &mut self,
        ready_repositories: &[String],
    ) -> Result<(), RepositoryRuntimeError> {
        self.tombstones
            .reconcile_ready_repositories(ready_repositories)?;
        self.persist_control_state()
    }

    pub(crate) fn replication_summary(
        &self,
    ) -> Result<RepositoryReplicaSummary, RepositoryRuntimeError> {
        Ok(RepositoryReplicaSummary {
            segment_ids: self
                .snapshot
                .segments
                .iter()
                .map(|segment| segment.id.clone())
                .collect(),
            partitions: self.partition_summaries()?,
            gaps: self.snapshot.gaps.iter().map(gap_summary).collect(),
            last_verified_unix_seconds: self.snapshot.last_verified_unix_seconds,
        })
    }

    pub(crate) fn missing_segment_ids(
        &self,
        remote: &RepositoryReplicaSummary,
        deep_verification: bool,
    ) -> Result<Vec<String>, RepositoryRuntimeError> {
        let local_ids = self
            .snapshot
            .segments
            .iter()
            .map(|segment| segment.id.as_str())
            .collect::<BTreeSet<_>>();
        let remote_ids = remote
            .segment_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let converged = if deep_verification {
            self.partition_summaries()? == remote.partitions
        } else {
            local_ids == remote_ids
        };
        if converged {
            return Ok(Vec::new());
        }
        Ok(remote
            .segment_ids
            .iter()
            .filter(|id| !local_ids.contains(id.as_str()))
            .take(MAX_REPAIR_SEGMENTS)
            .cloned()
            .collect())
    }

    pub(crate) fn repair_batch(
        &self,
        requested_segment_ids: &[String],
    ) -> Result<RepositoryRepairBatch, RepositoryRuntimeError> {
        if requested_segment_ids.len() > MAX_REPAIR_SEGMENTS {
            return Err(RepositoryRuntimeError::Replica(
                ReplicaError::RepairLimitExceeded,
            ));
        }
        let requested = requested_segment_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut response_bytes = 0usize;
        let mut segments = Vec::new();
        for segment in &self.snapshot.segments {
            if !requested.contains(segment.id.as_str()) {
                continue;
            }
            let next = segment.wire.len().saturating_add(128);
            if response_bytes.saturating_add(next) > MAX_RESPONSE_WIRE_BYTES {
                break;
            }
            response_bytes = response_bytes.saturating_add(next);
            segments.push(RepositoryReplicaSegment {
                identity: segment.identity.clone(),
                wire: segment.wire.clone(),
            });
        }
        Ok(RepositoryRepairBatch { segments })
    }

    pub(crate) fn relay_batch(&self) -> RepositoryRepairBatch {
        RepositoryRepairBatch {
            segments: self
                .snapshot
                .segments
                .iter()
                .rev()
                .take(MAX_REPAIR_SEGMENTS)
                .cloned()
                .map(|segment| RepositoryReplicaSegment {
                    identity: segment.identity,
                    wire: segment.wire,
                })
                .collect(),
        }
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
        self.storage
            .write(
                crate::state::history_storage::REPOSITORY_REPLICA_KEY,
                &bytes,
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
    }

    fn partition_summaries(
        &self,
    ) -> Result<Vec<RepositoryPartitionSummary>, RepositoryRuntimeError> {
        let mut summaries = self
            .snapshot
            .segments
            .iter()
            .map(partition_summary)
            .collect::<Result<Vec<_>, _>>()?;
        summaries.sort_by(|left, right| {
            (
                &left.source_node_id,
                left.source_epoch,
                &left.stream,
                left.partition,
                left.first_sequence,
            )
                .cmp(&(
                    &right.source_node_id,
                    right.source_epoch,
                    &right.stream,
                    right.partition,
                    right.first_sequence,
                ))
        });
        Ok(summaries)
    }
}

fn partition_summary(
    stored: &super::StoredSegment,
) -> Result<RepositoryPartitionSummary, RepositoryRuntimeError> {
    let segment = crate::history_sync::SignedSegment::from_wire(&stored.wire)?;
    let canonical = segment.canonical();
    let first = canonical.first_cursor();
    let last = canonical.last_cursor();
    let mut partition_hash = sha2::Sha256::new();
    use sha2::Digest as _;
    partition_hash.update(first.source_node_id().as_bytes());
    partition_hash.update(first.source_epoch().to_be_bytes());
    partition_hash.update(first.stream().as_bytes());
    partition_hash.update(first.sequence().to_be_bytes());
    partition_hash.update(last.sequence().to_be_bytes());
    let partition_hash = partition_hash.finalize();
    Ok(RepositoryPartitionSummary {
        source_node_id: first.source_node_id().to_owned(),
        source_epoch: first.source_epoch(),
        stream: first.stream().to_owned(),
        partition: u32::from_be_bytes([
            partition_hash[0],
            partition_hash[1],
            partition_hash[2],
            partition_hash[3],
        ]),
        first_sequence: first.sequence(),
        last_sequence: last.sequence(),
        hash: segment.segment_hash()?,
        record_count: u64::try_from(canonical.records().len())
            .map_err(|_| RepositoryRuntimeError::Replica(ReplicaError::InvalidRange))?,
    })
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
