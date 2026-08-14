use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    history_sync::{
        Acceptance, Cursor, ProtocolError, SchemaCatalog, SegmentReceiver,
        SegmentReceiverCheckpoint, SignedSegment, SyncRecord,
    },
    state::{
        history_repository::identity::RepositoryNodeIdentity,
        history_repository::{
            HistoryStorage,
            control::{HistoryWriteAvailability, RepositoryCapacity},
            query::{
                HistoryQuery, QueryCandidate, QueryCoverage, QueryError, QueryGap, QueryPlan,
                QueryRange, QuerySelector, StreamWatermark,
            },
        },
        history_storage::REPOSITORY_REPLICA_KEY,
    },
};

use super::{
    ReplicaCursor, ReplicaError, ReplicaFreshness, ReplicaRecord, TombstoneLedger,
    TombstoneLedgerCheckpoint,
};

const MAX_REPOSITORY_RECORDS: usize = 16_384;
const MAX_REPOSITORY_SEGMENTS: usize = 1_024;
const MAX_RUNTIME_STATE_BYTES: usize = 16 * 1024 * 1024;
const MAX_QUERY_RESPONSE_BYTES: usize = 256 * 1024;
const TOMBSTONE_HORIZON_SECONDS: u64 = 2 * 365 * 24 * 60 * 60;
const KNOWN_SCHEMAS: [(&str, u32); 6] = [
    ("runtime.v1", 1),
    ("path_health.v1", 1),
    ("traffic.v1", 1),
    ("connections.v1", 1),
    ("ip_usage.v1", 1),
    ("tombstone.v1", 1),
];

#[derive(Debug)]
pub(crate) enum RepositoryRuntimeError {
    Protocol(ProtocolError),
    Replica(ReplicaError),
    Query(QueryError),
    Storage(String),
    ClusterBindingMismatch,
    WriteStopped(HistoryWriteAvailability),
    StateLimitExceeded,
}

impl std::fmt::Display for RepositoryRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "history repository runtime error: {self:?}")
    }
}

impl std::error::Error for RepositoryRuntimeError {}

impl From<ProtocolError> for RepositoryRuntimeError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<ReplicaError> for RepositoryRuntimeError {
    fn from(value: ReplicaError) -> Self {
        Self::Replica(value)
    }
}

impl From<QueryError> for RepositoryRuntimeError {
    fn from(value: QueryError) -> Self {
        Self::Query(value)
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RepositorySyncReceipt {
    acknowledgement: RepositoryWatermark,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap: Option<RepositoryGap>,
    unknown_schema_records: usize,
    history_write_availability: HistoryWriteAvailability,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tombstone_acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
}

impl RepositorySyncReceipt {
    pub(crate) fn tombstone_acknowledgements(&self) -> &[RepositoryTombstoneAcknowledgement] {
        &self.tombstone_acknowledgements
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryTombstoneAcknowledgement {
    key: super::ReplicaRecordKey,
    repository_id: String,
}

impl RepositoryTombstoneAcknowledgement {
    pub(crate) fn repository_id(&self) -> &str {
        &self.repository_id
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RepositoryWatermark {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RepositoryGap {
    requested: RepositoryWatermark,
    earliest_available: RepositoryWatermark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryHistoryRecord {
    observed_at_unix_seconds: u64,
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    sequence: u64,
    subject_node_id: String,
    observer_node_id: String,
    schema_id: String,
    schema_version: u32,
    record_key: Vec<u8>,
    payload: Vec<u8>,
    tombstone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryHistoryQueryResponse {
    #[serde(flatten)]
    plan: QueryPlan,
    records: Vec<RepositoryHistoryRecord>,
    records_truncated: bool,
}

impl RepositoryHistoryQueryResponse {
    pub(crate) fn plan(&self) -> &QueryPlan {
        &self.plan
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LocalQueryMetadata {
    observed_start_unix_seconds: u64,
    observed_end_unix_seconds: u64,
    clock_skew_seconds: i64,
}

impl LocalQueryMetadata {
    pub(crate) fn current_window(now_unix_seconds: u64) -> Self {
        Self {
            observed_start_unix_seconds: now_unix_seconds.saturating_sub(5 * 60),
            observed_end_unix_seconds: now_unix_seconds,
            clock_skew_seconds: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepositoryReplicaSnapshot {
    #[serde(default)]
    cluster_id: Option<String>,
    #[serde(default)]
    receiver: Option<SegmentReceiverCheckpoint>,
    #[serde(default)]
    tombstones: TombstoneLedgerCheckpoint,
    #[serde(default)]
    records: Vec<StoredRecord>,
    #[serde(default)]
    segments: Vec<StoredSegment>,
    #[serde(default)]
    gaps: Vec<StoredGap>,
    #[serde(default)]
    capacity: RepositoryCapacity,
    #[serde(default)]
    last_verified_unix_seconds: Option<u64>,
    #[serde(default)]
    last_anti_entropy_unix_seconds: Option<u64>,
    #[serde(default)]
    last_deep_verification_unix_seconds: Option<u64>,
    #[serde(default)]
    relay_private_key: Option<[u8; 32]>,
    #[serde(default)]
    last_dynamic_relay_attempt_unix_seconds: Option<u64>,
}

impl Default for RepositoryReplicaSnapshot {
    fn default() -> Self {
        Self {
            cluster_id: None,
            receiver: None,
            tombstones: TombstoneLedger::new(TOMBSTONE_HORIZON_SECONDS).checkpoint(),
            records: Vec::new(),
            segments: Vec::new(),
            gaps: Vec::new(),
            capacity: RepositoryCapacity::default(),
            last_verified_unix_seconds: None,
            last_anti_entropy_unix_seconds: None,
            last_deep_verification_unix_seconds: None,
            relay_private_key: None,
            last_dynamic_relay_attempt_unix_seconds: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredRecord {
    observed_at_unix_seconds: u64,
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    sequence: u64,
    subject_node_id: String,
    observer_node_id: String,
    schema_id: String,
    schema_version: u32,
    record_key: Vec<u8>,
    payload: Vec<u8>,
    tombstone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSegment {
    id: String,
    identity: RepositoryNodeIdentity,
    wire: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredGap {
    #[serde(default)]
    source_node_id: String,
    #[serde(default)]
    source_epoch: u64,
    #[serde(default)]
    stream: String,
    #[serde(default)]
    first_sequence: u64,
    #[serde(default)]
    last_sequence: u64,
    start_unix_seconds: u64,
    end_unix_seconds: u64,
    permanent: bool,
}

pub(crate) struct RepositoryReplicaRuntime {
    storage: HistoryStorage,
    snapshot: RepositoryReplicaSnapshot,
    receiver: Option<SegmentReceiver>,
    tombstones: TombstoneLedger,
    #[cfg(test)]
    capacity_override: Option<(u64, u64)>,
}

impl std::fmt::Debug for RepositoryReplicaRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RepositoryReplicaRuntime")
            .field("cluster_id", &self.snapshot.cluster_id)
            .field("records", &self.snapshot.records.len())
            .finish()
    }
}

impl RepositoryReplicaRuntime {
    pub(crate) fn load(storage: HistoryStorage) -> Result<Self, RepositoryRuntimeError> {
        let snapshot: RepositoryReplicaSnapshot = storage
            .read(REPOSITORY_REPLICA_KEY)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .unwrap_or_default();
        let receiver = match (&snapshot.cluster_id, snapshot.receiver.clone()) {
            (Some(cluster_id), Some(checkpoint)) => Some(SegmentReceiver::from_checkpoint(
                cluster_id.clone(),
                known_schemas(),
                checkpoint,
            )?),
            _ => None,
        };
        let tombstones = TombstoneLedger::from_checkpoint(snapshot.tombstones.clone())?;
        Ok(Self {
            storage,
            snapshot,
            receiver,
            tombstones,
            #[cfg(test)]
            capacity_override: None,
        })
    }

    pub(crate) fn empty(storage: HistoryStorage) -> Self {
        Self {
            storage,
            snapshot: RepositoryReplicaSnapshot::default(),
            receiver: None,
            tombstones: TombstoneLedger::new(TOMBSTONE_HORIZON_SECONDS),
            #[cfg(test)]
            capacity_override: None,
        }
    }

    pub(crate) fn receive_wire(
        &mut self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wire: &[u8],
        now_unix_seconds: u64,
    ) -> Result<RepositorySyncReceipt, RepositoryRuntimeError> {
        self.receive_wire_from_repository(
            cluster_id,
            identity,
            wire,
            now_unix_seconds,
            &["local".to_owned()],
            "local",
        )
    }

    pub(crate) fn receive_wire_from_repository(
        &mut self,
        cluster_id: &str,
        identity: &RepositoryNodeIdentity,
        wire: &[u8],
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<RepositorySyncReceipt, RepositoryRuntimeError> {
        self.rebuild_if_stale(now_unix_seconds)?;
        self.refresh_capacity()?;
        let availability = self.snapshot.capacity.history_write_availability();
        if !availability.allows_history_writes() {
            return Err(RepositoryRuntimeError::WriteStopped(availability));
        }
        let segment = SignedSegment::from_wire(wire)?;
        self.bind_cluster(cluster_id)?;
        self.ensure_receiver()?;
        if self.snapshot.records.len() >= MAX_REPOSITORY_RECORDS {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let previous_receiver = self
            .receiver
            .as_ref()
            .expect("receiver initialized")
            .checkpoint()?;
        let previous_snapshot = self.snapshot.clone();
        let expired_tombstones = self.expire_tombstones(now_unix_seconds)?;
        let acceptance = self
            .receiver
            .as_mut()
            .expect("receiver initialized")
            .accept(&segment, identity);
        let acceptance = match acceptance {
            Ok(acceptance) => acceptance,
            Err(error) => {
                let records_gap = matches!(
                    error,
                    ProtocolError::SequenceGap { .. }
                        | ProtocolError::EpochGap { .. }
                        | ProtocolError::ForkDetected { .. }
                );
                if records_gap {
                    self.record_gap(
                        segment.canonical(),
                        matches!(
                            error,
                            ProtocolError::EpochGap { .. } | ProtocolError::ForkDetected { .. }
                        ),
                    );
                }
                if records_gap || expired_tombstones {
                    self.persist_or_restore(&previous_receiver, &previous_snapshot)?;
                }
                return Err(error.into());
            }
        };
        if matches!(acceptance, Acceptance::Duplicate { .. }) {
            return Ok(sync_receipt(acceptance, availability, Vec::new()));
        }

        if acceptance.gap().is_some() {
            self.record_gap(segment.canonical(), true);
        }
        let tombstone_acknowledgements = match self.append_known_records(
            segment.canonical(),
            now_unix_seconds,
            ready_repositories,
            local_repository_id,
        ) {
            Ok(acknowledgements) => acknowledgements,
            Err(error) => {
                self.restore(&previous_receiver, previous_snapshot)?;
                return Err(error);
            }
        };
        if let Err(error) = self.store_segment(identity, wire) {
            self.restore(&previous_receiver, previous_snapshot)?;
            return Err(error);
        }
        self.clear_repaired_gaps(segment.canonical());
        self.prune_retention(now_unix_seconds);
        self.snapshot.last_verified_unix_seconds = Some(now_unix_seconds);
        self.persist_or_restore(&previous_receiver, &previous_snapshot)?;
        Ok(sync_receipt(
            acceptance,
            availability,
            tombstone_acknowledgements,
        ))
    }

    pub(crate) fn query(
        &self,
        repository_id: &str,
        query: HistoryQuery,
        local: LocalQueryMetadata,
    ) -> Result<RepositoryHistoryQueryResponse, RepositoryRuntimeError> {
        let local_coverage = QueryCoverage::new(
            QueryRange::new(
                local.observed_start_unix_seconds,
                local.observed_end_unix_seconds,
            )?,
            QueryRange::new(
                local.observed_start_unix_seconds,
                local.observed_end_unix_seconds,
            )?,
        );
        let local_gap = QueryGap::new(
            query.range().start_unix_seconds(),
            query.range().end_unix_seconds(),
            false,
        )?;
        let local_candidate = QueryCandidate::local(
            local_coverage,
            std::iter::empty(),
            [local_gap],
            local.clock_skew_seconds,
        )?;
        let mut candidates = vec![local_candidate];
        if let Some(coverage) = self.repository_coverage() {
            let watermarks = self
                .receiver
                .as_ref()
                .map(SegmentReceiver::continuous_watermarks)
                .unwrap_or_default()
                .into_iter()
                .map(watermark_from_cursor)
                .collect::<Result<Vec<_>, _>>()?;
            let gaps = self
                .snapshot
                .gaps
                .iter()
                .map(|gap| {
                    QueryGap::new(gap.start_unix_seconds, gap.end_unix_seconds, gap.permanent)
                })
                .collect::<Result<Vec<_>, _>>()?;
            candidates.push(QueryCandidate::ready(
                repository_id,
                coverage,
                watermarks,
                gaps,
                0,
            )?);
        }
        let plan = QuerySelector::select(&query, candidates)?;
        let (records, records_truncated) = if plan.repository_id().is_some() {
            self.records_for(&query)
        } else {
            Default::default()
        };
        Ok(RepositoryHistoryQueryResponse {
            plan,
            records,
            records_truncated,
        })
    }

    pub(crate) fn query_local_only(
        &self,
        query: HistoryQuery,
        local: LocalQueryMetadata,
    ) -> Result<RepositoryHistoryQueryResponse, RepositoryRuntimeError> {
        let coverage = QueryCoverage::new(
            QueryRange::new(
                local.observed_start_unix_seconds,
                local.observed_end_unix_seconds,
            )?,
            QueryRange::new(
                local.observed_start_unix_seconds,
                local.observed_end_unix_seconds,
            )?,
        );
        let gap = QueryGap::new(
            query.range().start_unix_seconds(),
            query.range().end_unix_seconds(),
            false,
        )?;
        let plan = QuerySelector::select(
            &query,
            [QueryCandidate::local(
                coverage,
                std::iter::empty(),
                [gap],
                local.clock_skew_seconds,
            )?],
        )?;
        Ok(RepositoryHistoryQueryResponse {
            plan,
            records: Vec::new(),
            records_truncated: false,
        })
    }

    pub(crate) fn history_write_availability(&self) -> HistoryWriteAvailability {
        self.snapshot.capacity.history_write_availability()
    }

    pub(crate) fn control_plane_permitted(&self) -> bool {
        self.snapshot
            .capacity
            .history_write_availability()
            .allows_control_plane_operations()
    }

    pub(crate) fn relay_keypair(
        &mut self,
    ) -> Result<crate::history_sync::RelayKeypair, RepositoryRuntimeError> {
        let private_key = *self
            .snapshot
            .relay_private_key
            .get_or_insert_with(rand::random::<[u8; 32]>);
        self.persist_control_state()?;
        Ok(crate::history_sync::RelayKeypair::from_private_key(
            private_key,
        ))
    }

    pub(crate) fn begin_dynamic_relay_attempt(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<bool, RepositoryRuntimeError> {
        let jitter_seconds = self
            .snapshot
            .cluster_id
            .as_deref()
            .map(|cluster_id| u64::from(Sha256::digest(cluster_id.as_bytes())[0]) % (5 * 60))
            .unwrap_or_default();
        let due = self
            .snapshot
            .last_dynamic_relay_attempt_unix_seconds
            .is_none_or(|last| now_unix_seconds.saturating_sub(last) >= 60 * 60 + jitter_seconds);
        if due {
            self.snapshot.last_dynamic_relay_attempt_unix_seconds = Some(now_unix_seconds);
            self.persist_control_state()?;
        }
        Ok(due)
    }

    #[cfg(test)]
    pub(crate) fn force_capacity_for_test(
        &mut self,
        used_bytes: u64,
        filesystem_available_bytes: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        self.snapshot
            .capacity
            .record_usage(used_bytes, filesystem_available_bytes)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        #[cfg(test)]
        {
            self.capacity_override = Some((used_bytes, filesystem_available_bytes));
        }
        Ok(())
    }

    fn bind_cluster(&mut self, cluster_id: &str) -> Result<(), RepositoryRuntimeError> {
        match self.snapshot.cluster_id.as_deref() {
            Some(existing) if existing != cluster_id => {
                Err(RepositoryRuntimeError::ClusterBindingMismatch)
            }
            Some(_) => Ok(()),
            None => {
                self.snapshot.cluster_id = Some(cluster_id.to_owned());
                Ok(())
            }
        }
    }

    fn ensure_receiver(&mut self) -> Result<(), RepositoryRuntimeError> {
        if self.receiver.is_none() {
            let cluster_id = self
                .snapshot
                .cluster_id
                .clone()
                .ok_or(RepositoryRuntimeError::ClusterBindingMismatch)?;
            self.receiver = Some(SegmentReceiver::for_cluster(cluster_id, known_schemas()));
        }
        Ok(())
    }

    fn append_known_records(
        &mut self,
        segment: &crate::history_sync::CanonicalSegment,
        now_unix_seconds: u64,
        ready_repositories: &[String],
        local_repository_id: &str,
    ) -> Result<Vec<RepositoryTombstoneAcknowledgement>, RepositoryRuntimeError> {
        let mut acknowledgements = Vec::new();
        for (offset, record) in segment.records().iter().enumerate() {
            if !is_known_schema(record) {
                continue;
            }
            let sequence = segment
                .first_cursor()
                .sequence()
                .checked_add(
                    u64::try_from(offset)
                        .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
                )
                .ok_or(RepositoryRuntimeError::StateLimitExceeded)?;
            let cursor = ReplicaCursor::new(
                segment.first_cursor().source_node_id(),
                segment.first_cursor().source_epoch(),
                segment.first_cursor().stream(),
                sequence,
            )?;
            let (schema_id, schema_version) = record.schema();
            let replica_record = ReplicaRecord::new(
                &cursor,
                record.subject_node_id(),
                record.observer_node_id(),
                schema_id,
                schema_version,
                record.record_key().to_vec(),
                record.payload_bytes().to_vec(),
            )?;
            let key = replica_record.key();
            if record.is_tombstone() {
                self.tombstones
                    .tombstone(key, now_unix_seconds, ready_repositories)?;
                self.tombstones
                    .acknowledge(replica_record.key(), local_repository_id)?;
                acknowledgements.push(RepositoryTombstoneAcknowledgement {
                    key: replica_record.key(),
                    repository_id: local_repository_id.to_owned(),
                });
            } else if !self.tombstones.allows(&key) {
                return Err(RepositoryRuntimeError::Protocol(
                    ProtocolError::ResurrectionPrevented,
                ));
            }
            self.snapshot.records.push(StoredRecord::from_record(
                segment.closed_at_unix_seconds(),
                &cursor,
                record,
            ));
            if self.snapshot.records.len() > MAX_REPOSITORY_RECORDS {
                return Err(RepositoryRuntimeError::StateLimitExceeded);
            }
        }
        Ok(acknowledgements)
    }

    fn store_segment(
        &mut self,
        identity: &RepositoryNodeIdentity,
        wire: &[u8],
    ) -> Result<(), RepositoryRuntimeError> {
        let id = hex::encode(Sha256::digest(wire));
        if self
            .snapshot
            .segments
            .iter()
            .any(|segment| segment.id == id)
        {
            return Ok(());
        }
        if self.snapshot.segments.len() == MAX_REPOSITORY_SEGMENTS {
            self.snapshot.segments.remove(0);
        }
        self.snapshot.segments.push(StoredSegment {
            id,
            identity: identity.clone(),
            wire: wire.to_vec(),
        });
        Ok(())
    }

    fn record_gap(&mut self, segment: &crate::history_sync::CanonicalSegment, permanent: bool) {
        if self.snapshot.gaps.len() == 64 {
            self.snapshot.gaps.remove(0);
        }
        self.snapshot.gaps.push(StoredGap {
            source_node_id: segment.first_cursor().source_node_id().to_owned(),
            source_epoch: segment.first_cursor().source_epoch(),
            stream: segment.first_cursor().stream().to_owned(),
            first_sequence: segment.first_cursor().sequence(),
            last_sequence: segment.last_cursor().sequence(),
            start_unix_seconds: segment.opened_at_unix_seconds(),
            end_unix_seconds: segment.closed_at_unix_seconds(),
            permanent,
        });
    }

    fn clear_repaired_gaps(&mut self, segment: &crate::history_sync::CanonicalSegment) {
        self.snapshot.gaps.retain(|gap| {
            gap.permanent
                || gap.source_node_id != segment.first_cursor().source_node_id()
                || gap.source_epoch != segment.first_cursor().source_epoch()
                || gap.stream != segment.first_cursor().stream()
                || segment.first_cursor().sequence() > gap.first_sequence
                || segment.last_cursor().sequence() < gap.last_sequence
        });
    }

    fn prune_retention(&mut self, now_unix_seconds: u64) {
        let policy = super::RepositoryRetentionPolicy::default();
        let mut retained = BTreeMap::<RetentionBucket, RetainedRecord>::new();
        for record in self.snapshot.records.drain(..) {
            if record.tombstone {
                retained.insert(
                    RetentionBucket::tombstone(&record),
                    RetainedRecord::Raw(record),
                );
                continue;
            }
            let age = now_unix_seconds.saturating_sub(record.observed_at_unix_seconds);
            let Some(resolution) = policy.resolution_for_age(age) else {
                continue;
            };
            let preserves_minute_detail = policy.keeps_minute_detail(age);
            let bucket = RetentionBucket::for_record(&record, resolution, preserves_minute_detail);
            match retained.entry(bucket) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    if preserves_minute_detail {
                        entry.insert(RetainedRecord::Raw(record));
                    } else {
                        entry.insert(RetainedRecord::Aggregate(RetentionAggregate::from_record(
                            record,
                            resolution,
                            self.snapshot.cluster_id.as_deref(),
                        )));
                    }
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => match entry.get_mut() {
                    RetainedRecord::Raw(existing) => {
                        if record.sequence >= existing.sequence {
                            *existing = record;
                        }
                    }
                    RetainedRecord::Aggregate(aggregate) => aggregate.add(record),
                },
            }
        }
        self.snapshot.records = retained
            .into_values()
            .map(RetainedRecord::into_stored_record)
            .collect();
    }

    fn repository_coverage(&self) -> Option<QueryCoverage> {
        let start = self
            .snapshot
            .records
            .iter()
            .map(|record| record.observed_at_unix_seconds)
            .min()?;
        let end = self
            .snapshot
            .records
            .iter()
            .map(|record| record.observed_at_unix_seconds)
            .max()?;
        let range = QueryRange::new(start, end).expect("record timestamps are ordered");
        Some(QueryCoverage::new(range, range))
    }

    fn records_for(&self, query: &HistoryQuery) -> (Vec<RepositoryHistoryRecord>, bool) {
        let mut records = Vec::with_capacity(query.page_size());
        let mut response_bytes = 0usize;
        let mut truncated = false;
        for record in self.snapshot.records.iter().filter(|record| {
            record.observed_at_unix_seconds >= query.range().start_unix_seconds()
                && record.observed_at_unix_seconds <= query.range().end_unix_seconds()
        }) {
            let next_bytes = record
                .payload
                .len()
                .saturating_add(record.record_key.len())
                .saturating_add(256);
            if records.len() == query.page_size()
                || response_bytes.saturating_add(next_bytes) > MAX_QUERY_RESPONSE_BYTES
            {
                truncated = true;
                break;
            }
            response_bytes = response_bytes.saturating_add(next_bytes);
            records.push(record.clone().into());
        }
        (records, truncated)
    }

    fn refresh_capacity(&mut self) -> Result<(), RepositoryRuntimeError> {
        #[cfg(test)]
        let capacity = self.capacity_override;
        #[cfg(not(test))]
        let capacity: Option<(u64, u64)> = None;
        let (used_bytes, available) = match capacity {
            Some(capacity) => capacity,
            None => (
                self.serialized_snapshot_len()?,
                self.storage
                    .available_bytes()
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?,
            ),
        };
        self.snapshot
            .capacity
            .record_usage(used_bytes, available)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
    }

    fn persist_or_restore(
        &mut self,
        previous_receiver: &SegmentReceiverCheckpoint,
        previous_snapshot: &RepositoryReplicaSnapshot,
    ) -> Result<(), RepositoryRuntimeError> {
        self.snapshot.receiver = Some(
            self.receiver
                .as_ref()
                .expect("receiver initialized")
                .checkpoint()?,
        );
        self.snapshot.tombstones = self.tombstones.checkpoint();
        let bytes = serde_json::to_vec(&self.snapshot)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if bytes.len() > MAX_RUNTIME_STATE_BYTES {
            self.restore(previous_receiver, previous_snapshot.clone())?;
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        if let Err(error) = self.storage.write(REPOSITORY_REPLICA_KEY, &bytes) {
            self.restore(previous_receiver, previous_snapshot.clone())?;
            return Err(RepositoryRuntimeError::Storage(error.to_string()));
        }
        Ok(())
    }

    fn restore(
        &mut self,
        previous_receiver: &SegmentReceiverCheckpoint,
        previous_snapshot: RepositoryReplicaSnapshot,
    ) -> Result<(), RepositoryRuntimeError> {
        let cluster_id = self
            .snapshot
            .cluster_id
            .clone()
            .ok_or(RepositoryRuntimeError::ClusterBindingMismatch)?;
        self.receiver = Some(SegmentReceiver::from_checkpoint(
            cluster_id,
            known_schemas(),
            previous_receiver.clone(),
        )?);
        self.snapshot = previous_snapshot;
        self.tombstones = TombstoneLedger::from_checkpoint(self.snapshot.tombstones.clone())?;
        Ok(())
    }

    fn rebuild_if_stale(&mut self, now_unix_seconds: u64) -> Result<(), RepositoryRuntimeError> {
        let Some(last_verified) = self.snapshot.last_verified_unix_seconds else {
            return Ok(());
        };
        if !ReplicaFreshness::new(last_verified, TOMBSTONE_HORIZON_SECONDS)
            .requires_rebuild(now_unix_seconds)
        {
            return Ok(());
        }
        let cluster_id = self.snapshot.cluster_id.clone();
        self.snapshot = RepositoryReplicaSnapshot {
            cluster_id: cluster_id.clone(),
            ..RepositoryReplicaSnapshot::default()
        };
        self.tombstones = TombstoneLedger::new(TOMBSTONE_HORIZON_SECONDS);
        self.receiver =
            cluster_id.map(|cluster_id| SegmentReceiver::for_cluster(cluster_id, known_schemas()));
        Ok(())
    }

    fn expire_tombstones(&mut self, now_unix_seconds: u64) -> Result<bool, RepositoryRuntimeError> {
        let expired = self.tombstones.expire(now_unix_seconds);
        let removed_tombstones = !expired.is_empty();
        for key in expired {
            let cursor = Cursor::new(key.source_node_id(), key.source_epoch(), key.stream(), 0)?;
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
            if let Some(receiver) = self.receiver.as_mut() {
                receiver.forget_tombstone(&cursor, &record);
            }
            self.snapshot
                .records
                .retain(|record| !record.matches_key(&key));
        }
        Ok(removed_tombstones)
    }

    fn serialized_snapshot_len(&self) -> Result<u64, RepositoryRuntimeError> {
        serde_json::to_vec(&self.snapshot)
            .map(|bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
    }
}

impl StoredRecord {
    fn from_record(
        observed_at_unix_seconds: u64,
        cursor: &ReplicaCursor,
        record: &SyncRecord,
    ) -> Self {
        let (schema_id, schema_version) = record.schema();
        Self {
            observed_at_unix_seconds,
            source_node_id: cursor.source_node_id().to_owned(),
            source_epoch: cursor.source_epoch(),
            stream: cursor.stream().to_owned(),
            sequence: cursor.sequence(),
            subject_node_id: record.subject_node_id().to_owned(),
            observer_node_id: record.observer_node_id().to_owned(),
            schema_id: schema_id.to_owned(),
            schema_version,
            record_key: record.record_key().to_vec(),
            payload: record.payload_bytes().to_vec(),
            tombstone: record.is_tombstone(),
        }
    }

    fn matches_key(&self, key: &super::ReplicaRecordKey) -> bool {
        let (schema_id, schema_version) = key.schema();
        self.source_node_id == key.source_node_id()
            && self.source_epoch == key.source_epoch()
            && self.stream == key.stream()
            && self.subject_node_id == key.subject_node_id()
            && self.observer_node_id == key.observer_node_id()
            && self.schema_id == schema_id
            && self.schema_version == schema_version
            && self.record_key == key.record_key()
            && self.tombstone
    }
}

impl From<StoredRecord> for RepositoryHistoryRecord {
    fn from(record: StoredRecord) -> Self {
        Self {
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RetentionBucket {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    subject_node_id: String,
    observer_node_id: String,
    schema_id: String,
    schema_version: u32,
    record_key: Option<Vec<u8>>,
    bucket_start: u64,
}

impl RetentionBucket {
    fn for_record(
        record: &StoredRecord,
        resolution: super::RetentionResolution,
        preserves_minute_detail: bool,
    ) -> Self {
        let seconds = match resolution {
            super::RetentionResolution::Minute => 60,
            super::RetentionResolution::FiveMinutes => 5 * 60,
            super::RetentionResolution::Hour => 60 * 60,
        };
        Self::with_bucket(
            record,
            record.observed_at_unix_seconds / seconds * seconds,
            preserves_minute_detail,
        )
    }

    fn tombstone(record: &StoredRecord) -> Self {
        Self::with_bucket(record, record.observed_at_unix_seconds, true)
    }

    fn with_bucket(
        record: &StoredRecord,
        bucket_start: u64,
        preserves_minute_detail: bool,
    ) -> Self {
        Self {
            source_node_id: record.source_node_id.clone(),
            source_epoch: record.source_epoch,
            stream: record.stream.clone(),
            subject_node_id: record.subject_node_id.clone(),
            observer_node_id: record.observer_node_id.clone(),
            schema_id: record.schema_id.clone(),
            schema_version: record.schema_version,
            record_key: preserves_minute_detail.then(|| record.record_key.clone()),
            bucket_start,
        }
    }
}

enum RetainedRecord {
    Raw(StoredRecord),
    Aggregate(RetentionAggregate),
}

impl RetainedRecord {
    fn into_stored_record(self) -> StoredRecord {
        match self {
            Self::Raw(record) => record,
            Self::Aggregate(aggregate) => aggregate.into_stored_record(),
        }
    }
}

#[derive(Serialize)]
struct RetentionAggregatePayload {
    algorithm: &'static str,
    resolution: &'static str,
    record_count: u64,
    first_sequence: u64,
    last_sequence: u64,
    payload_sha256: String,
    complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    anonymized_identifier: Option<String>,
}

struct RetentionAggregate {
    first: StoredRecord,
    resolution: super::RetentionResolution,
    cluster_id: Option<String>,
    record_count: u64,
    first_sequence: u64,
    last_sequence: u64,
    payload_hasher: Sha256,
}

impl RetentionAggregate {
    fn from_record(
        record: StoredRecord,
        resolution: super::RetentionResolution,
        cluster_id: Option<&str>,
    ) -> Self {
        let mut payload_hasher = Sha256::new();
        payload_hasher.update(&record.payload);
        payload_hasher.update(&record.record_key);
        Self {
            first_sequence: record.sequence,
            last_sequence: record.sequence,
            first: record,
            resolution,
            cluster_id: cluster_id.map(ToOwned::to_owned),
            record_count: 1,
            payload_hasher,
        }
    }

    fn add(&mut self, record: StoredRecord) {
        self.first_sequence = self.first_sequence.min(record.sequence);
        self.last_sequence = self.last_sequence.max(record.sequence);
        self.record_count = self.record_count.saturating_add(1);
        self.payload_hasher.update(&record.payload);
        self.payload_hasher.update(&record.record_key);
        if record.sequence >= self.first.sequence {
            self.first = record;
        }
    }

    fn into_stored_record(self) -> StoredRecord {
        let mut record = self.first;
        let resolution = match self.resolution {
            super::RetentionResolution::Minute => "minute",
            super::RetentionResolution::FiveMinutes => "five_minutes",
            super::RetentionResolution::Hour => "hour",
        };
        let is_ip_history = record.schema_id == "ip_usage.v1";
        let anonymized_identifier = is_ip_history.then(|| {
            anonymized_identifier(
                self.cluster_id.as_deref(),
                &record.subject_node_id,
                &record.record_key,
            )
        });
        let payload = RetentionAggregatePayload {
            algorithm: "sha256",
            resolution,
            record_count: self.record_count,
            first_sequence: self.first_sequence,
            last_sequence: self.last_sequence,
            payload_sha256: hex::encode(self.payload_hasher.finalize()),
            complete: true,
            anonymized_identifier,
        };
        record.sequence = self.last_sequence;
        record.record_key = aggregate_record_key(&record, resolution);
        record.payload = serde_json::to_vec(&payload).expect("retention aggregate is serializable");
        record
    }
}

fn aggregate_record_key(record: &StoredRecord, resolution: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"xp-history-repository-aggregate-v1\0");
    hasher.update(record.source_node_id.as_bytes());
    hasher.update(record.source_epoch.to_be_bytes());
    hasher.update(record.stream.as_bytes());
    hasher.update(record.schema_id.as_bytes());
    hasher.update(resolution.as_bytes());
    hasher.update(record.observed_at_unix_seconds.to_be_bytes());
    hasher.finalize().to_vec()
}

fn anonymized_identifier(
    cluster_id: Option<&str>,
    subject_node_id: &str,
    raw_identifier: &[u8],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"xp-history-repository-ip-anonymization-v1\0");
    hasher.update(cluster_id.unwrap_or_default().as_bytes());
    hasher.update(subject_node_id.as_bytes());
    hasher.update(raw_identifier);
    hex::encode(hasher.finalize())
}

fn known_schemas() -> SchemaCatalog {
    SchemaCatalog::new(
        KNOWN_SCHEMAS
            .iter()
            .map(|(schema, version)| ((*schema).to_owned(), *version)),
    )
}

fn is_known_schema(record: &SyncRecord) -> bool {
    KNOWN_SCHEMAS.contains(&record.schema())
}

fn watermark_from_cursor(cursor: Cursor) -> Result<StreamWatermark, RepositoryRuntimeError> {
    Ok(StreamWatermark::new(
        cursor.source_node_id(),
        cursor.source_epoch(),
        cursor.stream(),
        cursor.sequence(),
    )?)
}

fn sync_receipt(
    acceptance: Acceptance,
    history_write_availability: HistoryWriteAvailability,
    tombstone_acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
) -> RepositorySyncReceipt {
    let acknowledgement =
        repository_watermark_from_cursor(acceptance.acknowledgement().watermark());
    let gap = acceptance.gap().map(|gap| RepositoryGap {
        requested: repository_watermark_from_cursor(gap.requested()),
        earliest_available: repository_watermark_from_cursor(gap.earliest_available()),
    });
    RepositorySyncReceipt {
        acknowledgement,
        gap,
        unknown_schema_records: acceptance.unknown_schema_records(),
        history_write_availability,
        tombstone_acknowledgements,
    }
}

fn repository_watermark_from_cursor(cursor: &Cursor) -> RepositoryWatermark {
    RepositoryWatermark {
        source_node_id: cursor.source_node_id().to_owned(),
        source_epoch: cursor.source_epoch(),
        stream: cursor.stream().to_owned(),
        sequence: cursor.sequence(),
    }
}

mod sync;
pub(crate) use sync::{RepositoryRepairBatch, RepositoryReplicaSummary};

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
