use std::collections::{BTreeMap, BTreeSet};

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
            RepositoryHistorySegmentRow, RepositoryHistoryTombstone, RepositoryReplicaMutation,
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

pub(crate) use backfill::RepositoryTieredBackfillRecord;
pub(crate) use error::RepositoryRuntimeError;
pub(crate) use receive::{PendingRepositoryMutation, source_stream_for_schema};
pub(crate) use status::RepositoryRuntimeStatus;
pub(crate) use sync::RepositoryReplicaGap;

const MAX_RUNTIME_STATE_BYTES: usize = 16 * 1024 * 1024;
// A continuation is persisted in the 16 MiB control snapshot. Keep its bounded per-bucket
// aggregate state comfortably below that limit even when source records approach their payload
// budget.
const RETENTION_COMPACTION_PAGE_SIZE: usize = 256;
const RETENTION_COMPACTION_BUCKET_LOOKAHEAD: usize = 32;
const REPLICATION_SEGMENT_PAGE_SIZE: usize = 256;
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
    #[serde(default)]
    local_history_backfill_inflight: Option<LocalHistoryBackfillInFlight>,
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
            local_history_backfill_inflight: None,
            initial_peer_backfills: BTreeMap::new(),
            retention_compaction_cursor: None,
            retention_compaction_continuation: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalHistoryBackfillInFlight {
    page_cursor: Option<String>,
    completed: bool,
    segment_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredRecord {
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
pub(crate) struct StoredSegment {
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
    #[serde(default)]
    aggregates: Vec<StoredRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    aggregate: Option<StoredRecord>,
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
    #[serde(default)]
    pub(crate) summary_cursor: Option<String>,
    #[serde(default)]
    pub(crate) summary_pending_segment_ids: Vec<String>,
    #[serde(default)]
    pub(crate) summary_pending_next_cursor: Option<String>,
    #[serde(default)]
    pub(crate) summary_complete: bool,
    #[serde(default)]
    pub(crate) summary_requires_tiered_backfill: bool,
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
    storage_degraded: bool,
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
            storage_degraded: false,
            #[cfg(test)]
            capacity_override: None,
        };
        if let Err(error) = runtime.migrate_history_to_sqlite() {
            if runtime.snapshot.external_history || !runtime.storage.degrade_to_json() {
                return Err(error);
            }
            tracing::warn!(
                error = %error,
                "repository history migration failed; continuing with JSON history"
            );
        }
        runtime.migrate_legacy_segment_cursor_index()?;
        Ok(runtime)
    }

    pub(crate) fn empty(storage: HistoryStorage) -> Self {
        Self {
            storage,
            snapshot: RepositoryReplicaSnapshot::default(),
            receiver: None,
            tombstones: TombstoneLedger::new(TOMBSTONE_HORIZON_SECONDS),
            storage_degraded: false,
            #[cfg(test)]
            capacity_override: None,
        }
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
        if !self.storage_degraded {
            self.prepare_for_replication(now_unix_seconds)?;
            self.refresh_capacity()?;
        }
        Ok(RepositoryRuntimeStatus {
            storage_mode: if self.storage_degraded {
                "sqlite_degraded".to_owned()
            } else if self.storage.is_sqlite() {
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

    pub(crate) fn local_history_backfill_cursor(&self) -> Option<&str> {
        self.snapshot.local_history_backfill_cursor.as_deref()
    }

    pub(crate) fn repair_legacy_tombstone_metadata(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<bool, RepositoryRuntimeError> {
        if !self.tombstones.repair_epoch_zero_metadata(now_unix_seconds) {
            return Ok(false);
        }
        self.persist_control_state()?;
        Ok(true)
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
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .get(peer_node_id)
            .cloned()
            .unwrap_or_default();
        self.snapshot.initial_peer_backfills.insert(
            peer_node_id.to_owned(),
            InitialPeerBackfillCheckpoint {
                page_cursor,
                stream_state,
                saw_history,
                completed,
                epoch: checkpoint.epoch,
                summary_cursor: checkpoint.summary_cursor,
                summary_pending_segment_ids: checkpoint.summary_pending_segment_ids,
                summary_pending_next_cursor: checkpoint.summary_pending_next_cursor,
                summary_complete: checkpoint.summary_complete,
                summary_requires_tiered_backfill: checkpoint.summary_requires_tiered_backfill,
            },
        );
        self.persist_control_state()
    }

    pub(crate) fn update_initial_peer_summary_checkpoint(
        &mut self,
        peer_node_id: &str,
        summary_cursor: Option<String>,
        pending_segment_ids: Vec<String>,
        pending_next_cursor: Option<String>,
        summary_complete: bool,
        summary_requires_tiered_backfill: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        let checkpoint = self
            .snapshot
            .initial_peer_backfills
            .entry(peer_node_id.to_owned())
            .or_default();
        checkpoint.summary_cursor = summary_cursor;
        checkpoint.summary_pending_segment_ids = pending_segment_ids;
        checkpoint.summary_pending_next_cursor = pending_next_cursor;
        checkpoint.summary_complete = summary_complete;
        checkpoint.summary_requires_tiered_backfill = summary_requires_tiered_backfill;
        self.persist_control_state()
    }

    /// Restarts a peer export after its source-side immutable export lease expires. The import
    /// epoch is intentionally preserved: already accepted segments are replayed as duplicates,
    /// then the receiver resumes at the first previously unseen segment without creating a
    /// second representation of the same historical rows.
    pub(crate) fn restart_initial_peer_backfill(
        &mut self,
        peer_node_id: &str,
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
                epoch,
                ..InitialPeerBackfillCheckpoint::default()
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
        let result = self.storage.allocate_repository_source_epoch(
            cluster_id,
            &allocator_node_id,
            crate::state::history_repository::replica::source_epoch(cluster_id, &allocator_node_id),
        );
        let epoch = self.finish_storage_write(result)?;
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
}

mod backfill;
#[cfg(test)]
#[path = "runtime/backfill_order_tests.rs"]
mod backfill_order_tests;
mod base64_bytes;
mod capacity;
mod helpers;
#[cfg(test)]
#[path = "runtime/legacy_cursor_tests.rs"]
mod legacy_cursor_tests;
mod paths;
mod query;
mod receive;
mod retention;
pub(crate) mod source;
mod storage;
mod sync;
use helpers::{
    is_known_schema, known_schemas, serialized_response_overhead, sync_receipt,
    watermark_from_cursor,
};
pub(crate) use source::source_epoch;
pub(crate) use sync::{RepositoryRepairBatch, RepositoryReplicaSegment, RepositoryReplicaSummary};

#[cfg(test)]
#[path = "runtime/repair_selection_tests.rs"]
mod repair_selection_tests;
#[cfg(test)]
#[path = "runtime/sqlite_order_tests.rs"]
mod sqlite_order_tests;
#[cfg(test)]
#[path = "runtime/sync_tests.rs"]
mod sync_tests;
#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "runtime/retention_tests.rs"]
mod retention_tests;

#[cfg(test)]
#[path = "runtime/repair_tests.rs"]
mod repair_tests;

#[cfg(test)]
#[path = "runtime/page_replay_tests.rs"]
mod page_replay_tests;

#[cfg(test)]
#[path = "runtime/query_budget_tests.rs"]
mod query_budget_tests;
#[cfg(test)]
#[path = "runtime/query_tests.rs"]
mod query_tests;
