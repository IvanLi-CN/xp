use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    history_sync::{
        Acceptance, CanonicalSegment, Cursor, ProtocolError, SegmentReceiver,
        SegmentReceiverCheckpoint, SignedSegment, SyncRecord,
    },
    state::{
        history_repository::identity::RepositoryNodeIdentity,
        history_repository::{
            HistoryStorage,
            control::{HistoryWriteAvailability, RepositoryCapacity},
            query::{
                HistoryQuery, QueryCandidate, QueryCoverage, QueryGap, QueryPlan, QueryRange,
                QuerySelector,
            },
        },
        history_storage::{
            REPOSITORY_REPLICA_KEY, RepositoryHistoryCompactionCursor, RepositoryHistoryRecordRow,
            RepositoryHistorySegmentRow, RepositoryHistoryTombstone,
        },
    },
};

use super::{
    ReplicaCursor, ReplicaFreshness, ReplicaRecord, ReplicaRecordKey, TombstoneLedger,
    TombstoneLedgerCheckpoint,
};
use paths::PeerDirectPathState;
use source::LocalSourceState;

mod error;
mod status;

pub(crate) use error::RepositoryRuntimeError;
pub(crate) use status::RepositoryRuntimeStatus;
pub(crate) use sync::RepositoryReplicaGap;

const MAX_RUNTIME_STATE_BYTES: usize = 16 * 1024 * 1024;
const RETENTION_COMPACTION_PAGE_SIZE: usize = 4_096;
const RETENTION_COMPACTION_BUCKET_LOOKAHEAD: usize = 64;
const REPLICATION_SEGMENT_PAGE_SIZE: usize = 256;

#[derive(Debug, Clone)]
pub(crate) struct RepositoryTieredBackfillRecord {
    pub(crate) observed_at_unix_seconds: u64,
    pub(crate) subject_node_id: String,
    pub(crate) observer_node_id: String,
    pub(crate) schema_id: String,
    pub(crate) schema_version: u32,
    pub(crate) record_key: Vec<u8>,
    pub(crate) payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryTieredBackfillPage {
    pub(crate) records: Vec<RepositoryTieredBackfillRecord>,
    pub(crate) next_cursor: Option<String>,
}
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryWatermark {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(with = "base64_bytes")]
    record_key: Vec<u8>,
    #[serde(with = "base64_bytes")]
    payload: Vec<u8>,
    tombstone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryHistoryQueryResponse {
    #[serde(flatten)]
    plan: QueryPlan,
    records: Vec<RepositoryHistoryRecord>,
    records_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page_cursor: Option<String>,
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
    /// Versioned marker for history held in the SQLite row store instead of this control blob.
    #[serde(default)]
    external_history: bool,
    #[serde(default)]
    gaps: Vec<StoredGap>,
    #[serde(default)]
    history_truncated: bool,
    #[serde(default)]
    capacity: RepositoryCapacity,
    #[serde(default)]
    last_verified_unix_seconds: Option<u64>,
    #[serde(default)]
    last_anti_entropy_unix_seconds: Option<u64>,
    #[serde(default)]
    last_deep_verification_unix_seconds: Option<u64>,
    #[serde(default)]
    last_dynamic_relay_attempt_unix_seconds: Option<u64>,
    #[serde(default)]
    local_source: LocalSourceState,
    #[serde(default)]
    peer_direct_paths: BTreeMap<String, PeerDirectPathState>,
    #[serde(default)]
    collector_failure_cycles: BTreeMap<String, u8>,
    #[serde(default)]
    source_last_received_unix_seconds: BTreeMap<String, u64>,
    #[serde(default)]
    tombstone_acknowledgement_cursor: Option<ReplicaRecordKey>,
    #[serde(default)]
    #[serde(rename = "relay_segment_offsets")]
    relay_segment_cursors: BTreeMap<String, RelaySegmentCursor>,
    #[serde(default)]
    replication_peer_offset: usize,
    #[serde(default)]
    deep_verified_peer_ids: BTreeSet<String>,
    #[serde(default)]
    local_history_backfill_completed: bool,
    #[serde(default)]
    local_history_backfill_cursor: Option<String>,
    /// Peer imports are checkpointed outside the raw history rows. A failed first-repository
    /// catch-up can therefore resume the signed import chain rather than replaying page zero
    /// with a forked cursor.
    #[serde(default)]
    initial_peer_backfills: BTreeMap<String, InitialPeerBackfillCheckpoint>,
    #[serde(default)]
    retention_compaction_cursor: Option<RetentionCompactionCursor>,
    #[serde(default)]
    retention_compaction_continuation: Option<RetentionCompactionContinuation>,
}

impl Default for RepositoryReplicaSnapshot {
    fn default() -> Self {
        Self {
            cluster_id: None,
            receiver: None,
            tombstones: TombstoneLedger::new(TOMBSTONE_HORIZON_SECONDS).checkpoint(),
            records: Vec::new(),
            segments: Vec::new(),
            external_history: false,
            gaps: Vec::new(),
            history_truncated: false,
            capacity: RepositoryCapacity::default(),
            last_verified_unix_seconds: None,
            last_anti_entropy_unix_seconds: None,
            last_deep_verification_unix_seconds: None,
            last_dynamic_relay_attempt_unix_seconds: None,
            local_source: LocalSourceState::default(),
            peer_direct_paths: BTreeMap::new(),
            collector_failure_cycles: BTreeMap::new(),
            source_last_received_unix_seconds: BTreeMap::new(),
            tombstone_acknowledgement_cursor: None,
            relay_segment_cursors: BTreeMap::new(),
            replication_peer_offset: 0,
            deep_verified_peer_ids: BTreeSet::new(),
            local_history_backfill_completed: false,
            local_history_backfill_cursor: None,
            initial_peer_backfills: BTreeMap::new(),
            retention_compaction_cursor: None,
            retention_compaction_continuation: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredRecord {
    observed_at_unix_seconds: u64,
    #[serde(default)]
    received_at_unix_seconds: u64,
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
    #[serde(default)]
    closed_at_unix_seconds: u64,
    identity: RepositoryNodeIdentity,
    wire: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum RelaySegmentCursor {
    LegacyOffset(usize),
    NextSegmentId(String),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetentionCompactionCursor {
    observed_start_unix_seconds: u64,
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetentionCompactionContinuation {
    aggregate: StoredRecord,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitialPeerBackfillCheckpoint {
    #[serde(default)]
    pub(crate) page_cursor: Option<String>,
    #[serde(default)]
    pub(crate) stream_state: BTreeMap<String, (u64, Option<[u8; 32]>)>,
    #[serde(default)]
    pub(crate) saw_history: bool,
    #[serde(default)]
    pub(crate) completed: bool,
    #[serde(default)]
    pub(crate) epoch: u64,
}

impl From<&RetentionCompactionCursor> for RepositoryHistoryCompactionCursor {
    fn from(cursor: &RetentionCompactionCursor) -> Self {
        Self {
            observed_start_unix_seconds: cursor.observed_start_unix_seconds,
            source_node_id: cursor.source_node_id.clone(),
            source_epoch: cursor.source_epoch,
            stream: cursor.stream.clone(),
            sequence: cursor.sequence,
        }
    }
}

impl From<&RepositoryHistoryRecordRow> for RetentionCompactionCursor {
    fn from(row: &RepositoryHistoryRecordRow) -> Self {
        Self {
            observed_start_unix_seconds: row.observed_start_unix_seconds,
            source_node_id: row.source_node_id.clone(),
            source_epoch: row.source_epoch,
            stream: row.stream.clone(),
            sequence: row.sequence,
        }
    }
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
        let mut runtime = Self {
            storage,
            snapshot,
            receiver,
            tombstones,
            #[cfg(test)]
            capacity_override: None,
        };
        runtime.migrate_history_to_sqlite()?;
        Ok(runtime)
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
        let previous_receiver = self
            .receiver
            .as_ref()
            .expect("receiver initialized")
            .checkpoint()?;
        let previous_snapshot = self.snapshot.clone();
        self.tombstones
            .reconcile_ready_repositories(ready_repositories)?;
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
            self.persist_or_restore(&previous_receiver, &previous_snapshot)?;
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
        self.record_source_received(segment.canonical(), now_unix_seconds)?;
        // A forward keyset cursor must survive continuous ingestion. New rows at or before the
        // completed cursor are handled by a bounded restart below; newer rows are reached by the
        // current pass without starving the rest of the retained window.
        self.reopen_retention_compaction_for_late_record(segment.canonical());
        self.prune_retention(now_unix_seconds)?;
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
        if let Some(coverage) = self.repository_coverage(query.subject_node_id())? {
            let watermarks = self
                .receiver
                .as_ref()
                .map(SegmentReceiver::continuous_watermarks)
                .unwrap_or_default()
                .into_iter()
                .map(watermark_from_cursor)
                .collect::<Result<Vec<_>, _>>()?;
            let mut gaps = self
                .snapshot
                .gaps
                .iter()
                .filter(|gap| {
                    query
                        .subject_node_id()
                        .is_none_or(|subject_node_id| gap.source_node_id == subject_node_id)
                })
                .map(|gap| {
                    QueryGap::new(gap.start_unix_seconds, gap.end_unix_seconds, gap.permanent)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if let Some((start, end)) = self.incomplete_aggregate_gap(&query)? {
                gaps.push(QueryGap::new(start, end, true)?);
            }
            if self.snapshot.history_truncated {
                gaps.push(QueryGap::new(
                    coverage.observed().start_unix_seconds(),
                    coverage.observed().end_unix_seconds(),
                    true,
                )?);
            }
            candidates.push(QueryCandidate::ready(
                repository_id,
                coverage,
                watermarks,
                gaps,
                0,
            )?);
        }
        let plan = QuerySelector::select(&query, candidates)?;
        let (records, records_truncated, next_page_cursor) = if plan.repository_id().is_some() {
            self.records_for(&query, &plan)?
        } else {
            (Vec::new(), false, None)
        };
        Ok(RepositoryHistoryQueryResponse {
            plan,
            records,
            records_truncated,
            next_page_cursor,
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
            next_page_cursor: None,
        })
    }

    pub(crate) fn runtime_status(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<RepositoryRuntimeStatus, RepositoryRuntimeError> {
        self.prepare_for_replication(now_unix_seconds)?;
        self.refresh_capacity()?;
        Ok(RepositoryRuntimeStatus {
            storage_mode: if self.storage.is_sqlite() {
                "sqlite".to_owned()
            } else {
                "degraded_json".to_owned()
            },
            capacity: self.snapshot.capacity.clone(),
            record_count: if self.uses_sqlite_history() {
                self.storage
                    .repository_history_record_count()
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            } else {
                self.snapshot.records.len()
            },
            segment_count: if self.uses_sqlite_history() {
                self.storage
                    .repository_history_segment_count()
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            } else {
                self.snapshot.segments.len()
            },
            gap_count: self.snapshot.gaps.len(),
            history_truncated: self.snapshot.history_truncated,
            last_verified_unix_seconds: self.snapshot.last_verified_unix_seconds,
            last_anti_entropy_unix_seconds: self.snapshot.last_anti_entropy_unix_seconds,
            last_deep_verification_unix_seconds: self.snapshot.last_deep_verification_unix_seconds,
            last_dynamic_relay_attempt_unix_seconds: self
                .snapshot
                .last_dynamic_relay_attempt_unix_seconds,
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

    pub(crate) fn local_history_backfill_completed(&self) -> bool {
        self.snapshot.local_history_backfill_completed
    }

    pub(crate) fn tiered_backfill_page(
        &self,
        page_cursor: Option<&str>,
        limit: usize,
    ) -> Result<RepositoryTieredBackfillPage, RepositoryRuntimeError> {
        let after = page_cursor
            .map(|cursor| {
                if cursor.len() > 1_024 {
                    return Err(RepositoryRuntimeError::StateLimitExceeded);
                }
                serde_json::from_slice::<RepositoryHistoryCompactionCursor>(
                    &URL_SAFE_NO_PAD
                        .decode(cursor)
                        .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)?,
                )
                .map_err(|_| RepositoryRuntimeError::StateLimitExceeded)
            })
            .transpose()?;
        let mut rows = self
            .storage
            .repository_history_records_page(after.as_ref(), limit.saturating_add(1))
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = has_more
            .then(|| {
                rows.last().map(|row| {
                    URL_SAFE_NO_PAD.encode(
                        serde_json::to_vec(&RepositoryHistoryCompactionCursor::from(row))
                            .expect("repository backfill cursor is serializable"),
                    )
                })
            })
            .flatten();
        let records = rows
            .into_iter()
            .map(StoredRecord::from_sqlite_row)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|record| RepositoryTieredBackfillRecord {
                observed_at_unix_seconds: record.observed_at_unix_seconds,
                subject_node_id: record.subject_node_id,
                observer_node_id: record.observer_node_id,
                schema_id: record.schema_id,
                schema_version: record.schema_version,
                record_key: record.record_key,
                payload: record.payload,
            })
            .collect();
        Ok(RepositoryTieredBackfillPage {
            records,
            next_cursor,
        })
    }

    pub(crate) fn local_history_backfill_cursor(&self) -> Option<&str> {
        self.snapshot.local_history_backfill_cursor.as_deref()
    }

    pub(crate) fn checkpoint_local_history_backfill(
        &mut self,
        page_cursor: Option<String>,
        completed: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        self.snapshot.local_history_backfill_cursor = page_cursor;
        self.snapshot.local_history_backfill_completed = completed;
        self.persist_control_state()
    }

    pub(crate) fn mark_local_history_backfill_completed(
        &mut self,
    ) -> Result<(), RepositoryRuntimeError> {
        self.checkpoint_local_history_backfill(None, true)
    }

    pub(crate) fn initial_peer_backfill_checkpoint(
        &self,
        peer_node_id: &str,
    ) -> Option<InitialPeerBackfillCheckpoint> {
        self.snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .cloned()
    }

    pub(crate) fn update_initial_peer_backfill_checkpoint(
        &mut self,
        peer_node_id: &str,
        page_cursor: Option<String>,
        stream_state: BTreeMap<String, (u64, Option<[u8; 32]>)>,
        saw_history: bool,
        completed: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        let epoch = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .map(|checkpoint| checkpoint.epoch)
            .unwrap_or_default();
        self.snapshot.initial_peer_backfills.insert(
            peer_node_id.to_owned(),
            InitialPeerBackfillCheckpoint {
                page_cursor,
                stream_state,
                saw_history,
                completed,
                epoch,
            },
        );
        self.persist_control_state()
    }

    pub(crate) fn initial_peer_backfill_epoch(
        &mut self,
        cluster_id: &str,
        source_node_id: &str,
        peer_node_id: &str,
    ) -> Result<u64, RepositoryRuntimeError> {
        if let Some(epoch) = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .map(|checkpoint| checkpoint.epoch)
            .filter(|epoch| *epoch != 0)
        {
            return Ok(epoch);
        }
        let allocator_node_id = format!("{source_node_id}:initial-backfill:{peer_node_id}");
        let epoch = self
            .storage
            .allocate_repository_source_epoch(
                cluster_id,
                &allocator_node_id,
                crate::state::history_repository::replica::source_epoch(
                    cluster_id,
                    &allocator_node_id,
                ),
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        self.snapshot
            .initial_peer_backfills
            .entry(peer_node_id.to_owned())
            .or_default()
            .epoch = epoch;
        self.persist_control_state()?;
        Ok(epoch)
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

    fn migrate_history_to_sqlite(&mut self) -> Result<(), RepositoryRuntimeError> {
        if !self.storage.is_sqlite() || self.snapshot.external_history {
            return Ok(());
        }
        if !self.snapshot.records.is_empty() {
            let rows = self
                .snapshot
                .records
                .iter()
                .map(StoredRecord::sqlite_row)
                .collect::<Result<Vec<_>, _>>()?;
            self.storage
                .upsert_repository_history_records(&rows)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        }
        if !self.snapshot.segments.is_empty() {
            let rows = self
                .snapshot
                .segments
                .iter()
                .map(StoredSegment::sqlite_row)
                .collect::<Result<Vec<_>, _>>()?;
            self.storage
                .upsert_repository_history_segments(&rows)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        }
        self.snapshot.records.clear();
        self.snapshot.segments.clear();
        self.snapshot.external_history = true;
        self.persist_control_state()
    }

    fn uses_sqlite_history(&self) -> bool {
        self.snapshot.external_history && self.storage.is_sqlite()
    }

    fn stored_segments_page(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredSegment>, RepositoryRuntimeError> {
        if !self.uses_sqlite_history() {
            let mut segments = self
                .snapshot
                .segments
                .iter()
                .filter(|segment| after_id.is_none_or(|after_id| segment.id.as_str() > after_id))
                .cloned()
                .collect::<Vec<_>>();
            segments.sort_by(|left, right| left.id.cmp(&right.id));
            segments.truncate(limit);
            return Ok(segments);
        }
        let mut segments = self
            .storage
            .repository_history_segments_page(after_id, limit)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .into_iter()
            .map(StoredSegment::from_sqlite_row)
            .collect::<Result<Vec<_>, _>>()?;
        segments.sort_by(|left, right| left.id.cmp(&right.id));
        segments.truncate(limit);
        Ok(segments)
    }

    fn stored_segments_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<StoredSegment>, RepositoryRuntimeError> {
        if !self.uses_sqlite_history() {
            let requested = ids.iter().collect::<BTreeSet<_>>();
            return Ok(self
                .snapshot
                .segments
                .iter()
                .filter(|segment| requested.contains(&segment.id))
                .cloned()
                .collect());
        }
        self.storage
            .repository_history_segments_by_ids(ids)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .into_iter()
            .map(StoredSegment::from_sqlite_row)
            .collect()
    }

    fn sqlite_records(
        &self,
        subject_node_id: Option<&str>,
        start_unix_seconds: Option<u64>,
        end_unix_seconds: Option<u64>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<StoredRecord>, RepositoryRuntimeError> {
        self.storage
            .repository_history_records(
                subject_node_id,
                start_unix_seconds,
                end_unix_seconds,
                offset,
                limit,
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
            .into_iter()
            .map(StoredRecord::from_sqlite_row)
            .collect()
    }

    fn incomplete_aggregate_gap(
        &self,
        query: &HistoryQuery,
    ) -> Result<Option<(u64, u64)>, RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            return self
                .storage
                .repository_history_incomplete_aggregate_range(
                    query.subject_node_id(),
                    query.range().start_unix_seconds(),
                    query.range().end_unix_seconds(),
                )
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()));
        }
        Ok(retention::incomplete_aggregate_gap(
            self.snapshot.records.iter().filter(|record| {
                query
                    .subject_node_id()
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
                    && {
                        let (start, end) = retention::record_time_range(record);
                        start <= query.range().end_unix_seconds()
                            && query.range().start_unix_seconds() <= end
                    }
            }),
        ))
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
            let (schema_id, schema_version) = record.schema();
            let cursor = ReplicaCursor::new(
                segment.first_cursor().source_node_id(),
                segment.first_cursor().source_epoch(),
                segment.first_cursor().stream(),
                sequence,
            )?;
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
                let target_stream = source_stream_for_schema(schema_id).ok_or_else(|| {
                    RepositoryRuntimeError::Storage(format!(
                        "tombstone schema has no target stream: {schema_id}"
                    ))
                })?;
                let target_cursor = ReplicaCursor::new(
                    segment.first_cursor().source_node_id(),
                    segment.first_cursor().source_epoch(),
                    target_stream,
                    sequence,
                )?;
                let target_key = ReplicaRecord::new(
                    &target_cursor,
                    record.subject_node_id(),
                    record.observer_node_id(),
                    schema_id,
                    schema_version,
                    record.record_key().to_vec(),
                    record.payload_bytes().to_vec(),
                )?
                .key();
                self.delete_records_for_tombstone(&target_key)?;
                self.tombstones
                    .tombstone(key, now_unix_seconds, ready_repositories)?;
                self.tombstones
                    .acknowledge(replica_record.key(), local_repository_id)?;
                acknowledgements.push(RepositoryTombstoneAcknowledgement {
                    key: replica_record.key(),
                    repository_id: local_repository_id.to_owned(),
                });
                // Tombstones live in the bounded ledger, never in the queryable SQLite record
                // store. This keeps a deletion from extending coverage or retaining payloads.
                continue;
            } else if !self.tombstones.allows(&key) {
                return Err(RepositoryRuntimeError::Protocol(
                    ProtocolError::ResurrectionPrevented,
                ));
            }
            let stored = StoredRecord::from_record(
                segment.closed_at_unix_seconds(),
                now_unix_seconds,
                &cursor,
                record,
            );
            if self.uses_sqlite_history() {
                self.storage
                    .upsert_repository_history_records(&[stored.sqlite_row()?])
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            } else {
                self.snapshot.records.push(stored);
            }
        }
        Ok(acknowledgements)
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

    fn reopen_retention_compaction_for_late_record(
        &mut self,
        segment: &crate::history_sync::CanonicalSegment,
    ) {
        let Some(cursor) = self.snapshot.retention_compaction_cursor.as_ref() else {
            return;
        };
        let first = segment.first_cursor();
        let incoming = (
            segment.opened_at_unix_seconds(),
            first.source_node_id(),
            first.source_epoch(),
            first.stream(),
            first.sequence(),
        );
        let completed = (
            cursor.observed_start_unix_seconds,
            cursor.source_node_id.as_str(),
            cursor.source_epoch,
            cursor.stream.as_str(),
            cursor.sequence,
        );
        if incoming <= completed {
            self.snapshot.retention_compaction_cursor = None;
        }
    }

    fn delete_records_for_tombstone(
        &mut self,
        key: &ReplicaRecordKey,
    ) -> Result<(), RepositoryRuntimeError> {
        let (schema_id, schema_version) = key.schema();
        let prefix = key.record_key().ends_with(b":");
        if self.uses_sqlite_history() {
            self.storage
                .delete_repository_history_for_tombstone(&RepositoryHistoryTombstone {
                    source_node_id: key.source_node_id().to_owned(),
                    source_epoch: key.source_epoch(),
                    stream: key.stream().to_owned(),
                    subject_node_id: key.subject_node_id().to_owned(),
                    observer_node_id: key.observer_node_id().to_owned(),
                    schema_id: schema_id.to_owned(),
                    schema_version,
                    record_key: key.record_key().to_vec(),
                    prefix,
                })
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        } else {
            self.snapshot
                .records
                .retain(|record| record.tombstone || !record.matches_tombstone_key(key, prefix));
        }
        Ok(())
    }

    fn prune_retention(&mut self, now_unix_seconds: u64) -> Result<(), RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            return self.prune_sqlite_retention(now_unix_seconds);
        }
        retention::prune_records(
            &mut self.snapshot.records,
            &self.snapshot.gaps,
            now_unix_seconds,
            self.snapshot.cluster_id.as_deref(),
        );
        // Signed wire payloads are only an anti-entropy cache. The two-year contract is carried
        // by compacted canonical SQLite rows; retaining minute-sized wire payloads for that full
        // period would defeat the repository quota before tiering can help.
        let repair_cache_cutoff = now_unix_seconds
            .saturating_sub(super::RepositoryRetentionPolicy::default().minute_retention_seconds());
        self.snapshot
            .segments
            .retain(|segment| segment.closed_at_unix_seconds >= repair_cache_cutoff);
        Ok(())
    }

    fn prune_sqlite_retention(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<(), RepositoryRuntimeError> {
        let policy = super::RepositoryRetentionPolicy::default();
        let cutoff = now_unix_seconds.saturating_sub(policy.minute_retention_seconds());
        let after = self
            .snapshot
            .retention_compaction_cursor
            .as_ref()
            .map(RepositoryHistoryCompactionCursor::from);
        let fetched_rows = self
            .storage
            .repository_history_records_for_compaction(
                cutoff,
                after.as_ref(),
                RETENTION_COMPACTION_PAGE_SIZE
                    .saturating_add(RETENTION_COMPACTION_BUCKET_LOOKAHEAD),
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        let fetched_row_count = fetched_rows.len();
        let mut rows = fetched_rows.clone();
        let mut has_more = rows.len()
            == RETENTION_COMPACTION_PAGE_SIZE.saturating_add(RETENTION_COMPACTION_BUCKET_LOOKAHEAD);
        if rows.len() > RETENTION_COMPACTION_PAGE_SIZE {
            let page_boundary = rows
                .last()
                .expect("nonempty retention lookahead page")
                .observed_start_unix_seconds;
            let closed_prefix_len = rows
                .iter()
                .rposition(|row| {
                    let record = StoredRecord::from_sqlite_row(row.clone())
                        .expect("SQLite compaction row was previously validated");
                    retention::compaction_bucket_end(&record, now_unix_seconds)
                        .is_none_or(|bucket_end| bucket_end < page_boundary)
                })
                .map_or(0, |index| index + 1);
            rows.truncate(closed_prefix_len);
        }
        has_more |= rows.len() < fetched_row_count;
        if rows.is_empty() {
            let fetched_records = fetched_rows
                .iter()
                .cloned()
                .map(StoredRecord::from_sqlite_row)
                .collect::<Result<Vec<_>, _>>()?;
            if fetched_records.len() == fetched_row_count
                && fetched_records.windows(2).all(|pair| {
                    retention::same_compaction_bucket(&pair[0], &pair[1], now_unix_seconds)
                })
            {
                rows = fetched_rows.clone();
                has_more = true;
            }
        }
        if rows.is_empty() {
            // No closed bucket fits in this bounded lookahead. Retain the cursor and wait for
            // later history rather than creating a partial aggregate or loading an unbounded
            // bucket into memory.
            return Ok(());
        }
        let mut removed_rows = rows.clone();
        let continuation = self.snapshot.retention_compaction_continuation.take();
        let continuation = continuation.filter(|continuation| {
            rows.first().is_some_and(|row| {
                StoredRecord::from_sqlite_row(row.clone())
                    .ok()
                    .is_some_and(|record| {
                        retention::same_compaction_bucket(
                            &continuation.aggregate,
                            &record,
                            now_unix_seconds,
                        )
                    })
            })
        });
        if let Some(continuation) = &continuation {
            removed_rows.push(continuation.aggregate.sqlite_row()?);
        }
        let mut records = rows
            .iter()
            .cloned()
            .map(StoredRecord::from_sqlite_row)
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(continuation) = &continuation {
            records.push(continuation.aggregate.clone());
        }
        let should_continue = rows.len() == fetched_row_count
            && has_more
            && records.windows(2).all(|pair| {
                retention::same_compaction_bucket(&pair[0], &pair[1], now_unix_seconds)
            });
        retention::prune_records(
            &mut records,
            &self.snapshot.gaps,
            now_unix_seconds,
            self.snapshot.cluster_id.as_deref(),
        );
        let retained = records
            .iter()
            .map(StoredRecord::sqlite_row)
            .collect::<Result<Vec<_>, _>>()?;
        if !rows.is_empty() {
            self.storage
                .replace_repository_history_records(&removed_rows, &retained)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        }
        self.snapshot.retention_compaction_cursor =
            if !has_more && rows.len() < RETENTION_COMPACTION_PAGE_SIZE {
                None
            } else {
                rows.last().map(RetentionCompactionCursor::from)
            };
        if should_continue {
            self.snapshot.retention_compaction_continuation = retained
                .first()
                .cloned()
                .and_then(|row| StoredRecord::from_sqlite_row(row).ok())
                .map(|aggregate| RetentionCompactionContinuation { aggregate });
        }
        self.storage
            .delete_repository_history_before(
                now_unix_seconds.saturating_sub(policy.max_age_seconds()),
                now_unix_seconds.saturating_sub(policy.minute_retention_seconds()),
            )
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        self.persist_control_state()
    }

    fn repository_coverage(
        &self,
        subject_node_id: Option<&str>,
    ) -> Result<Option<QueryCoverage>, RepositoryRuntimeError> {
        if self.uses_sqlite_history() {
            return self
                .storage
                .repository_history_coverage(subject_node_id)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
                .map(|coverage| {
                    Ok(QueryCoverage::new(
                        QueryRange::new(
                            coverage.observed_start_unix_seconds,
                            coverage.observed_end_unix_seconds,
                        )?,
                        QueryRange::new(
                            coverage.received_start_unix_seconds,
                            coverage.received_end_unix_seconds,
                        )?,
                    ))
                })
                .transpose();
        }
        let observed_start = self
            .snapshot
            .records
            .iter()
            .filter(|record| {
                subject_node_id
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
            })
            .map(|record| retention::record_time_range(record).0)
            .min();
        let Some(observed_start) = observed_start else {
            return Ok(None);
        };
        let observed_end = self
            .snapshot
            .records
            .iter()
            .filter(|record| {
                subject_node_id
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
            })
            .map(|record| retention::record_time_range(record).1)
            .max()
            .expect("repository coverage had a first record");
        let received_start = self
            .snapshot
            .records
            .iter()
            .filter(|record| {
                subject_node_id
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
            })
            .map(record_received_at)
            .min()
            .expect("repository coverage had a first record");
        let received_end = self
            .snapshot
            .records
            .iter()
            .filter(|record| {
                subject_node_id
                    .is_none_or(|subject_node_id| record.subject_node_id == subject_node_id)
            })
            .map(record_received_at)
            .max()
            .expect("repository coverage had a first record");
        Ok(Some(QueryCoverage::new(
            QueryRange::new(observed_start, observed_end).expect("record timestamps are ordered"),
            QueryRange::new(received_start, received_end).expect("record timestamps are ordered"),
        )))
    }

    fn refresh_capacity(&mut self) -> Result<(), RepositoryRuntimeError> {
        #[cfg(test)]
        let capacity = self.capacity_override;
        #[cfg(not(test))]
        let capacity: Option<(u64, u64)> = None;
        let (used_bytes, available) = match capacity {
            Some(capacity) => capacity,
            None => (
                if self.uses_sqlite_history() {
                    self.storage
                        .repository_history_used_bytes()
                        .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
                } else {
                    self.serialized_snapshot_len()?
                },
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
        let mut local_source = self.snapshot.local_source.clone();
        local_source.rotate_after_repository_rebuild();
        if let (Some(cluster_id), Some(node_id)) = (cluster_id.as_deref(), local_source.node_id()) {
            self.storage
                .record_repository_source_epoch(cluster_id, node_id, local_source.epoch())
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        }
        if self.storage.is_sqlite() {
            self.storage
                .clear_repository_history()
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        }
        self.snapshot = RepositoryReplicaSnapshot {
            cluster_id: cluster_id.clone(),
            external_history: self.storage.is_sqlite(),
            local_source,
            ..RepositoryReplicaSnapshot::default()
        };
        self.tombstones = TombstoneLedger::new(TOMBSTONE_HORIZON_SECONDS);
        self.receiver =
            cluster_id.map(|cluster_id| SegmentReceiver::for_cluster(cluster_id, known_schemas()));
        self.persist_control_state()
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
            if self.uses_sqlite_history() {
                let (schema_id, schema_version) = key.schema();
                self.storage
                    .delete_repository_history_tombstone(&RepositoryHistoryTombstone {
                        source_node_id: key.source_node_id().to_owned(),
                        source_epoch: key.source_epoch(),
                        stream: key.stream().to_owned(),
                        subject_node_id: key.subject_node_id().to_owned(),
                        observer_node_id: key.observer_node_id().to_owned(),
                        schema_id: schema_id.to_owned(),
                        schema_version,
                        record_key: key.record_key().to_vec(),
                        prefix: false,
                    })
                    .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
            }
        }
        Ok(removed_tombstones)
    }

    fn serialized_snapshot_len(&self) -> Result<u64, RepositoryRuntimeError> {
        serde_json::to_vec(&self.snapshot)
            .map(|bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
    }
}

fn record_received_at(record: &StoredRecord) -> u64 {
    if record.received_at_unix_seconds == 0 {
        record.observed_at_unix_seconds
    } else {
        record.received_at_unix_seconds
    }
}

fn source_stream_for_schema(schema_id: &str) -> Option<&'static str> {
    Some(match schema_id {
        "runtime.v1" => "runtime",
        "path_health.v1" => "path_health",
        "traffic.v1" => "traffic",
        "connections.v1" => "connections",
        "ip_usage.v1" => "ip_usage",
        _ => return None,
    })
}

impl StoredRecord {
    fn from_record(
        observed_at_unix_seconds: u64,
        received_at_unix_seconds: u64,
        cursor: &ReplicaCursor,
        record: &SyncRecord,
    ) -> Self {
        let (schema_id, schema_version) = record.schema();
        Self {
            observed_at_unix_seconds,
            received_at_unix_seconds,
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

    fn matches_tombstone_key(&self, key: &ReplicaRecordKey, prefix: bool) -> bool {
        let (schema_id, schema_version) = key.schema();
        (prefix || self.stream == key.stream())
            && self.subject_node_id == key.subject_node_id()
            && self.observer_node_id == key.observer_node_id()
            && self.schema_id == schema_id
            && self.schema_version == schema_version
            && if prefix {
                self.record_key.starts_with(key.record_key())
            } else {
                self.record_key == key.record_key()
            }
    }

    fn sqlite_row(&self) -> Result<RepositoryHistoryRecordRow, RepositoryRuntimeError> {
        let (observed_start_unix_seconds, observed_end_unix_seconds) =
            retention::record_time_range(self);
        let aggregate_metadata = retention::aggregate_metadata(self);
        Ok(RepositoryHistoryRecordRow {
            source_node_id: self.source_node_id.clone(),
            source_epoch: self.source_epoch,
            stream: self.stream.clone(),
            sequence: self.sequence,
            subject_node_id: self.subject_node_id.clone(),
            observer_node_id: self.observer_node_id.clone(),
            schema_id: self.schema_id.clone(),
            schema_version: self.schema_version,
            record_key: self.record_key.clone(),
            tombstone: self.tombstone,
            observed_start_unix_seconds,
            observed_end_unix_seconds,
            received_at_unix_seconds: record_received_at(self),
            aggregate_complete: aggregate_metadata.map(|(complete, _, _)| complete),
            aggregate_start_unix_seconds: aggregate_metadata.map(|(_, start, _)| start),
            aggregate_end_unix_seconds: aggregate_metadata.map(|(_, _, end)| end),
            payload: serde_json::to_vec(self)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?,
        })
    }

    fn from_sqlite_row(row: RepositoryHistoryRecordRow) -> Result<Self, RepositoryRuntimeError> {
        serde_json::from_slice(&row.payload)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))
    }
}

impl StoredSegment {
    fn sqlite_row(&self) -> Result<RepositoryHistorySegmentRow, RepositoryRuntimeError> {
        Ok(RepositoryHistorySegmentRow {
            id: self.id.clone(),
            closed_at_unix_seconds: self.closed_at_unix_seconds,
            payload: serde_json::to_vec(self)
                .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?,
        })
    }

    fn from_sqlite_row(row: RepositoryHistorySegmentRow) -> Result<Self, RepositoryRuntimeError> {
        let mut segment: Self = serde_json::from_slice(&row.payload)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?;
        if segment.closed_at_unix_seconds == 0 {
            segment.closed_at_unix_seconds = row.closed_at_unix_seconds;
        }
        Ok(segment)
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

mod base64_bytes;
mod capacity;
mod helpers;
mod paths;
mod query;
mod retention;
pub(crate) mod source;
mod sync;
use helpers::{
    is_known_schema, known_schemas, serialized_response_overhead, sync_receipt,
    watermark_from_cursor,
};
pub(crate) use source::source_epoch;
pub(crate) use sync::{RepositoryRepairBatch, RepositoryReplicaSegment, RepositoryReplicaSummary};

#[cfg(test)]
#[path = "runtime/sync_tests.rs"]
mod sync_tests;
#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "runtime/query_budget_tests.rs"]
mod query_budget_tests;
#[cfg(test)]
#[path = "runtime/query_tests.rs"]
mod query_tests;
