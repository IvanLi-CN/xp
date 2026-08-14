use axum::http::Method;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, time::Duration};
use tokio::time::MissedTickBehavior;

use crate::{
    control_plane_mesh::{MeshPeerTarget, peer_target_from_node},
    history_sync::{CanonicalSegment, Cursor, RelayFrame, RelayKeypair, SyncRecord},
    state::history_repository::{
        control::{
            RepositoryLifecycle, RepositoryMemberRuntimePatch, RepositoryMemberRuntimeUpdate,
        },
        identity::RepositoryNodeId,
        replica::{
            ReplicaWork, RepositoryRepairBatch, RepositoryReplicaSegment, RepositoryReplicaSummary,
            RepositorySyncReceipt, RepositoryTombstoneAcknowledgement, rendezvous_collectors,
        },
    },
};

use super::super::AppState;
use super::{
    INTERNAL_HISTORY_REPOSITORY_RELAY, RepositoryRelayRequest, RepositoryRepairRequest,
    RepositorySyncRequest, RepositoryTombstoneAcknowledgementRequest,
};

const REPOSITORY_REPLICATION_INTERVAL: Duration = Duration::from_secs(5 * 60);
const SOURCE_COLLECTION_INTERVAL: Duration = Duration::from_secs(60);
const REPOSITORY_REQUEST_BUDGET: Duration = Duration::from_secs(15);
const MAX_REPOSITORY_PEERS_PER_CYCLE: usize = 4;
const READY_STABILITY_WINDOW: Duration = Duration::from_secs(5 * 60);
const MAX_SOURCE_PAYLOAD_BYTES: usize = 32 * 1024;
const MAX_SOURCE_SUMMARY_ITEMS: usize = 64;
const MAX_INITIAL_BACKFILL_PAGE_BYTES: usize = 192 * 1024;
const MAX_INITIAL_BACKFILL_PAGE_RECORDS: usize = 64;
const CLUSTER_RELAY_KEY_CONTEXT: &[u8] = b"xp-history-repository-relay-key-v1\0";
struct SourceRecordBatch {
    records: Vec<SyncRecord>,
    deletion_markers: Vec<crate::node_history::RepositoryHistoryDeletionMarker>,
}

struct PeerBackfillImport<'a> {
    identity: &'a crate::state::history_repository::identity::RepositoryNodeIdentity,
    signing_key: &'a ed25519_dalek::SigningKey,
    peer_node_id: &'a str,
    epoch: u64,
    ready_repository_ids: &'a [String],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RepositoryInitialBackfillRecord {
    observed_at_unix_seconds: u64,
    subject_node_id: String,
    observer_node_id: String,
    schema_id: String,
    schema_version: u32,
    record_key_base64: String,
    payload_base64: String,
    tombstone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepositoryInitialBackfillPage {
    records: Vec<RepositoryInitialBackfillRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page_cursor: Option<String>,
}

struct HistoricalBackfillCollector {
    after: Option<HistoricalBackfillSortKey>,
    limit: usize,
    records: BTreeMap<HistoricalBackfillSortKey, (u64, SyncRecord)>,
    has_more: bool,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
struct HistoricalBackfillSortKey {
    observed_at_unix_seconds: u64,
    schema_id: String,
    #[serde(with = "backfill_cursor_key")]
    record_key: Vec<u8>,
}

impl HistoricalBackfillCollector {
    fn new(after: Option<HistoricalBackfillSortKey>, limit: usize) -> Self {
        Self {
            after,
            limit,
            records: BTreeMap::new(),
            has_more: false,
        }
    }

    fn push(&mut self, record: (u64, SyncRecord)) {
        let key = HistoricalBackfillSortKey {
            observed_at_unix_seconds: record.0,
            schema_id: record.1.schema().0.to_owned(),
            record_key: record.1.record_key().to_vec(),
        };
        if self.after.as_ref().is_some_and(|after| key <= *after) {
            return;
        }
        self.records.insert(key, record);
        if self.records.len() > self.limit {
            self.records.pop_last();
            self.has_more = true;
        }
    }

    fn next_cursor(&self) -> anyhow::Result<Option<String>> {
        self.has_more
            .then(|| {
                self.records
                    .last_key_value()
                    .expect("backfill cursor requires a record")
                    .0
                    .encode()
            })
            .transpose()
    }

    fn into_records(self) -> Vec<(u64, SyncRecord)> {
        self.records.into_values().collect()
    }
}

mod backfill_cursor_key {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(value))
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(serde::de::Error::custom)
    }
}

impl HistoricalBackfillSortKey {
    fn encode(&self) -> anyhow::Result<String> {
        Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(self)?))
    }

    fn decode(encoded: &str) -> anyhow::Result<Self> {
        if encoded.len() > 1_024 {
            anyhow::bail!("initial history backfill cursor exceeds limit");
        }
        Ok(serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded)?)?)
    }
}

impl RepositoryInitialBackfillRecord {
    fn into_sync_record(self) -> anyhow::Result<(u64, SyncRecord)> {
        Ok((
            self.observed_at_unix_seconds,
            SyncRecord::new(
                self.subject_node_id,
                self.observer_node_id,
                self.schema_id,
                self.schema_version,
                URL_SAFE_NO_PAD.decode(self.record_key_base64)?,
                URL_SAFE_NO_PAD.decode(self.payload_base64)?,
                self.tombstone,
            ),
        ))
    }
}

mod direct;
mod source;
pub(super) use direct::{
    all_cluster_peers, eligible_mesh_relay_peers, is_transport_failure, repository_direct_request,
    repository_mesh_request,
};
use source::{
    should_attempt_source_relay, source_record, source_record_with_key,
    source_record_with_key_for_subject,
};

pub(crate) fn spawn_repository_replica_worker(state: AppState) {
    let source_state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SOURCE_COLLECTION_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) = publish_local_history_segments(&source_state).await {
                tracing::debug!(error = %error, "history source collection cycle skipped");
            }
        }
    });
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(REPOSITORY_REPLICATION_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) = replicate_ready_repositories(&state).await {
                tracing::debug!(error = %error, "history repository replication cycle skipped");
            }
        }
    });
}

async fn replicate_ready_repositories(state: &AppState) -> anyhow::Result<()> {
    let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
    sync_local_repository_capacity(state, now).await?;
    advance_local_repository_lifecycle(state, now).await?;
    let Ok((ready_repository_ids, peers)) = ready_repository_peers(state).await else {
        return Ok(());
    };
    let known_source_node_ids = known_history_source_node_ids(state).await;
    if !ready_repository_ids
        .iter()
        .any(|id| id == &state.cluster.node_id)
    {
        return Ok(());
    }

    let (work, tombstone_acknowledgements) = {
        let mut runtime = state.repository_replica.lock().await;
        runtime.prepare_for_replication(now)?;
        runtime.reconcile_ready_repositories(&ready_repository_ids)?;
        runtime.record_stale_collection_cycles(
            now,
            &ready_repository_ids,
            &state.cluster.node_id,
            &known_source_node_ids,
        )?;
        (
            runtime.replication_work(now),
            runtime.tombstone_acknowledgement_page(&state.cluster.node_id)?,
        )
    };
    if !tombstone_acknowledgements.acknowledgements().is_empty() {
        match propagate_tombstone_acknowledgements(
            state,
            &ready_repository_ids,
            tombstone_acknowledgements.acknowledgements().to_vec(),
        )
        .await
        {
            Ok(()) => {
                state
                    .repository_replica
                    .lock()
                    .await
                    .record_tombstone_acknowledgement_delivery(
                        tombstone_acknowledgements.next_cursor(),
                    )?;
            }
            Err(error) => tracing::debug!(
                error = %error,
                "history repository tombstone acknowledgement retry failed"
            ),
        }
    }
    if !work.is_anti_entropy() {
        return Ok(());
    }

    let selected_peer_ids = state
        .repository_replica
        .lock()
        .await
        .next_replication_peers(
            &ready_repository_ids,
            &state.cluster.node_id,
            MAX_REPOSITORY_PEERS_PER_CYCLE,
        )?;
    let peers_to_replicate = selected_peer_ids
        .iter()
        .filter_map(|repository_id| peers.iter().find(|peer| peer.node_id == *repository_id))
        .collect::<Vec<_>>();
    let mut synchronized = false;
    let mut deep_verification_succeeded =
        work.is_deep_verification() && selected_peer_ids.is_empty();
    for peer in peers_to_replicate {
        match replicate_peer(state, peer, &ready_repository_ids, now, work, true).await {
            Ok(directly_converged) => {
                synchronized = true;
                if work.is_deep_verification() && directly_converged {
                    deep_verification_succeeded = state
                        .repository_replica
                        .lock()
                        .await
                        .record_direct_peer_deep_verification(
                            &peer.node_id,
                            &ready_repository_ids,
                            &state.cluster.node_id,
                            work,
                        )?;
                } else if work.is_deep_verification() {
                    deep_verification_succeeded = false;
                }
            }
            Err(error) => {
                if work.is_deep_verification() {
                    deep_verification_succeeded = false;
                }
                tracing::debug!(
                    peer = %peer.node_id,
                    error = %error,
                    "history repository peer replication failed"
                );
                if !is_transport_failure(&error) {
                    continue;
                }
                match replicate_peer_via_dynamic_relay(
                    state,
                    peer,
                    &peers,
                    &ready_repository_ids,
                    now,
                )
                .await
                {
                    Err(relay_error) => {
                        tracing::debug!(
                            peer = %peer.node_id,
                            error = %relay_error,
                            "history repository dynamic relay was unavailable"
                        );
                    }
                    Ok(()) => {
                        synchronized = true;
                    }
                }
            }
        }
    }
    if synchronized || peers.len() == 1 {
        let completed_work = completed_replication_work(work, deep_verification_succeeded);
        state
            .repository_replica
            .lock()
            .await
            .record_replication_completed(now, completed_work)?;
    }
    if work.is_deep_verification() {
        update_local_replica_convergence(state, deep_verification_succeeded).await?;
    }
    Ok(())
}

async fn publish_local_history_segments(state: &AppState) -> anyhow::Result<()> {
    let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
    let Ok((ready_repository_ids, peers)) = ready_repository_peers(state).await else {
        return Ok(());
    };
    publish_local_history_segment(state, &ready_repository_ids, &peers, now).await
}

async fn publish_local_history_segment(
    state: &AppState,
    ready_repository_ids: &[String],
    peers: &[MeshPeerTarget],
    now: u64,
) -> anyhow::Result<()> {
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let identity = super::derived_repository_identity(state, node_id)
        .map_err(|_| anyhow::anyhow!("derive local history source identity"))?;
    let signing_key = super::derived_repository_signing_key(state, identity.node_id().as_str())
        .map_err(|_| anyhow::anyhow!("derive local history source signing key"))?;
    let source_batch = source_records(state, now).await?;
    let (segments, gaps) = {
        let mut runtime = state.repository_replica.lock().await;
        let segments = runtime.queue_local_source_segments_for_repositories(
            &state.cluster.cluster_id,
            identity,
            &signing_key,
            source_batch.records,
            now,
            ready_repository_ids,
        )?;
        let gaps = runtime.local_source_backpressure_gaps(&state.cluster.node_id);
        (segments, gaps)
    };
    if segments.is_empty() {
        return Ok(());
    }
    let assignment = rendezvous_collectors(&state.cluster.node_id, ready_repository_ids)?;
    let primary_repository_id = assignment.primary().to_owned();
    let selected_repository_id = state
        .repository_replica
        .lock()
        .await
        .local_source_collector(&primary_repository_id, assignment.standby());
    let Some(selected_peer) = peers
        .iter()
        .find(|peer| peer.node_id == selected_repository_id)
    else {
        state
            .repository_replica
            .lock()
            .await
            .record_local_source_collector_delivery(
                &primary_repository_id,
                &selected_repository_id,
                false,
            )?;
        tracing::debug!(
            repository = selected_repository_id,
            "selected history repository is unreachable"
        );
        return Ok(());
    };
    let mut delivery_succeeded = true;
    let mut transport_failed = false;
    let mut tombstone_acknowledgements = Vec::new();
    for segment in &segments {
        if selected_repository_id == state.cluster.node_id {
            if let Err(error) =
                receive_local_source_segment(state, segment, &gaps, ready_repository_ids, now).await
            {
                delivery_succeeded = false;
                tracing::debug!(
                    repository = selected_repository_id,
                    error = %error,
                    "local history source segment remains queued for retry"
                );
            } else {
                state
                    .repository_replica
                    .lock()
                    .await
                    .acknowledge_local_source_segment(&segment.wire)?;
            }
        } else {
            let body = serde_json::to_vec(&RepositorySyncRequest::with_wire(
                segment.identity.clone(),
                &segment.wire,
                gaps.clone(),
            )?)?;
            match repository_direct_request::<RepositorySyncReceipt>(
                state,
                selected_peer,
                Method::POST,
                "/api/admin/_internal/history-repository/sync",
                body,
            )
            .await
            {
                Ok(receipt) => {
                    tombstone_acknowledgements
                        .extend(receipt.tombstone_acknowledgements().iter().cloned());
                    state
                        .repository_replica
                        .lock()
                        .await
                        .acknowledge_local_source_segment(&segment.wire)?;
                }
                Err(error) => {
                    transport_failed |= error.is_transport();
                    delivery_succeeded = false;
                    tracing::debug!(
                        repository = selected_repository_id,
                        error = %error,
                        "history source segment remains queued for retry"
                    );
                }
            }
        }
    }
    if !tombstone_acknowledgements.is_empty() {
        state
            .repository_replica
            .lock()
            .await
            .acknowledge_tombstones(&tombstone_acknowledgements)?;
    }
    // The collector fans its signed acknowledgement to every source and repository before it
    // returns the sync receipt. A non-repository source only records that local acknowledgement;
    // it must never impersonate a repository to fan it out itself.
    let acknowledgements_replicated = true;
    if should_attempt_source_relay(
        transport_failed,
        selected_repository_id == state.cluster.node_id,
    ) {
        let relay_peers = eligible_mesh_relay_peers(state).await;
        match relay_local_source_segments(
            state,
            selected_peer,
            &relay_peers,
            &state.cluster.node_id,
            now,
        )
        .await
        {
            Ok(()) => delivery_succeeded = true,
            Err(error) => tracing::debug!(
                repository = selected_repository_id,
                error = %error,
                "history source dynamic relay was unavailable"
            ),
        }
    }
    state
        .repository_replica
        .lock()
        .await
        .record_local_source_collector_delivery(
            &primary_repository_id,
            &selected_repository_id,
            delivery_succeeded,
        )?;
    let tombstones_fully_acknowledged = state
        .repository_replica
        .lock()
        .await
        .local_source_tombstones_fully_acknowledged(
            &state.cluster.node_id,
            &source_batch.deletion_markers,
        )?;
    if delivery_succeeded
        && !transport_failed
        && acknowledgements_replicated
        && tombstones_fully_acknowledged
    {
        for marker in &source_batch.deletion_markers {
            state
                .node_history
                .complete_repository_deletion_marker(&state.cluster.node_id, marker)
                .await;
        }
        state
            .repository_replica
            .lock()
            .await
            .complete_local_source_tombstones(&source_batch.deletion_markers)?;
    }
    Ok(())
}

async fn relay_local_source_segments(
    state: &AppState,
    target: &MeshPeerTarget,
    peers: &[MeshPeerTarget],
    source_node_id: &str,
    now: u64,
) -> anyhow::Result<()> {
    let payload = {
        let mut runtime = state.repository_replica.lock().await;
        if !runtime.begin_source_dynamic_relay_attempt(&state.cluster.cluster_id, now)? {
            return Err(anyhow::anyhow!("source dynamic relay attempt is not due"));
        }
        RepositoryRepairBatch {
            segments: runtime.local_source_pending_segments(),
            gaps: runtime.local_source_backpressure_gaps(source_node_id),
        }
        .frame_sized_relay_payload()?
    };
    if payload.batch.segments.is_empty() {
        return Ok(());
    }
    let relay = peers
        .iter()
        .find(|peer| peer.node_id != target.node_id && peer.node_id != source_node_id)
        .ok_or_else(|| anyhow::anyhow!("no eligible Mesh member can relay source history"))?;
    let keypair = cluster_relay_keypair(state, source_node_id)?;
    let target_public_key = cluster_relay_keypair(state, &target.node_id)?.public_key();
    let frame = RelayFrame::seal(
        keypair,
        target_public_key,
        rand::random(),
        &payload.bytes,
        target.node_id.as_bytes(),
    )?;
    let body = serde_json::to_vec(&RepositoryRelayRequest {
        target_repository_id: target.node_id.clone(),
        source_repository_id: source_node_id.to_owned(),
        relay_repository_id: None,
        frame,
    })?;
    repository_mesh_request::<serde_json::Value>(
        state,
        relay,
        Method::POST,
        INTERNAL_HISTORY_REPOSITORY_RELAY,
        body,
    )
    .await?;
    for segment in payload.batch.segments {
        state
            .repository_replica
            .lock()
            .await
            .acknowledge_local_source_segment(&segment.wire)?;
    }
    Ok(())
}

async fn receive_local_source_segment(
    state: &AppState,
    segment: &RepositoryReplicaSegment,
    gaps: &[crate::state::history_repository::replica::RepositoryReplicaGap],
    ready_repository_ids: &[String],
    now: u64,
) -> anyhow::Result<()> {
    let receipt = {
        let mut runtime = state.repository_replica.lock().await;
        if !gaps.is_empty() {
            runtime.merge_replica_gaps(gaps)?;
        }
        runtime.receive_wire_from_repository(
            &state.cluster.cluster_id,
            &segment.identity,
            &segment.wire,
            now,
            ready_repository_ids,
            &state.cluster.node_id,
        )?
    };
    if !receipt.tombstone_acknowledgements().is_empty() {
        propagate_tombstone_acknowledgements(
            state,
            ready_repository_ids,
            receipt.tombstone_acknowledgements().to_vec(),
        )
        .await?;
    }
    Ok(())
}

async fn source_records(state: &AppState, now: u64) -> anyhow::Result<SourceRecordBatch> {
    let runtime = state.node_runtime.snapshot(50).await;
    let history = state.node_history.snapshot(&state.cluster.node_id).await;
    let mesh = state.mesh_telemetry.snapshot().await;
    let deletion_markers = state
        .node_history
        .repository_deletion_markers(&state.cluster.node_id)
        .await;
    let (inbound_ip, connections) = {
        let store = state.store.lock().await;
        let inbound_ip = serde_json::json!({
            "generated_at": store.inbound_ip_usage().generated_at,
            "latest_minute": store.inbound_ip_usage().latest_minute,
            "online_stats_unavailable": store.inbound_ip_usage().online_stats_unavailable,
            "memberships": store.inbound_ip_usage().memberships.values()
                .filter(|membership| membership.node_id == state.cluster.node_id)
                .take(MAX_SOURCE_SUMMARY_ITEMS)
                .map(|membership| serde_json::json!({
                    "user_id": membership.user_id,
                    "endpoint_id": membership.endpoint_id,
                    "endpoint_tag": membership.endpoint_tag,
                    "ip_count": membership.ips.len(),
                    "last_seen_at": membership.ips.values()
                        .map(|record| record.last_seen_at.as_str())
                        .max(),
                }))
                .collect::<Vec<_>>(),
        });
        let connections = serde_json::json!({
            "generated_at": store.tcp_connection_usage().generated_at,
            "latest_minute": store.tcp_connection_usage().latest_minute,
            "linux_only": store.tcp_connection_usage().linux_only,
            "endpoints": store.tcp_connection_usage().endpoints.values()
                .filter(|endpoint| endpoint.node_id == state.cluster.node_id)
                .take(MAX_SOURCE_SUMMARY_ITEMS)
                .map(|endpoint| serde_json::json!({
                    "endpoint_id": endpoint.endpoint_id,
                    "endpoint_tag": endpoint.endpoint_tag,
                    "port": endpoint.port,
                    "count": endpoint.counts.last().copied().unwrap_or_default(),
                }))
                .collect::<Vec<_>>(),
        });
        (inbound_ip, connections)
    };
    let traffic = history.as_ref().map(|history| {
        serde_json::json!({
            "last_synced_at": history.last_synced_at,
            "last_sync_error": history.last_sync_error,
            "last_five_minute": history.traffic.as_ref()
                .and_then(|traffic| traffic.five_minute.last()),
            "last_daily": history.traffic.as_ref()
                .and_then(|traffic| traffic.daily.last()),
        })
    });
    let path_health = serde_json::json!({
        "generated_at": mesh.generated_at,
        "peers": mesh.peers.into_iter().take(MAX_SOURCE_SUMMARY_ITEMS).collect::<Vec<_>>(),
    });
    let mut live_records = [
        source_record(
            "runtime.v1",
            &state.cluster.node_id,
            now,
            serde_json::to_value(runtime)?,
            false,
        ),
        source_record(
            "traffic.v1",
            &state.cluster.node_id,
            now,
            traffic.unwrap_or(serde_json::Value::Null),
            false,
        ),
        source_record(
            "path_health.v1",
            &state.cluster.node_id,
            now,
            path_health,
            false,
        ),
        source_record(
            "ip_usage.v1",
            &state.cluster.node_id,
            now,
            inbound_ip,
            false,
        ),
        source_record(
            "connections.v1",
            &state.cluster.node_id,
            now,
            connections,
            false,
        ),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    if let Some(history) = history.as_ref() {
        for user_id in &history.user_traffic_users {
            live_records.push(source_record_with_key(
                "traffic.v1",
                &state.cluster.node_id,
                now,
                format!("node-history:user:{user_id}:current").into_bytes(),
                serde_json::json!({
                    "user_id": user_id,
                    "node_id": state.cluster.node_id,
                    "observed_at_unix_seconds": now,
                }),
                false,
            )?);
        }
    }
    let records = source_records_with_deletions(
        &state.cluster.node_id,
        now,
        deletion_markers
            .iter()
            .map(|marker| {
                (
                    marker.schema_id.clone(),
                    marker.record_key.clone(),
                    marker.target_node_id().map(str::to_owned),
                )
            })
            .collect(),
        live_records,
    )?;
    Ok(SourceRecordBatch {
        records,
        deletion_markers,
    })
}

fn source_records_with_deletions(
    node_id: &str,
    now: u64,
    deletion_markers: Vec<(String, Vec<u8>, Option<String>)>,
    live_records: Vec<SyncRecord>,
) -> anyhow::Result<Vec<SyncRecord>> {
    let mut tombstones = deletion_markers
        .into_iter()
        .map(|(schema_id, record_key, target_node_id)| {
            let target_node_id = target_node_id.as_deref().unwrap_or(node_id);
            source_record_with_key_for_subject(
                &schema_id,
                target_node_id,
                target_node_id,
                now,
                record_key,
                serde_json::json!({ "deleted_at_unix_seconds": now }),
                true,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    tombstones.extend(live_records);
    Ok(tombstones)
}

async fn sync_local_repository_capacity(state: &AppState, now: u64) -> anyhow::Result<()> {
    let capacity = state
        .repository_replica
        .lock()
        .await
        .runtime_status(now)?
        .capacity()
        .clone();
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let capacity_is_current = {
        let store = state.store.lock().await;
        store
            .state()
            .repository_membership
            .as_ref()
            .and_then(|membership| membership.repository(&node_id))
            .is_none_or(|member| member.capacity() == &capacity)
    };
    if capacity_is_current {
        return Ok(());
    }
    super::super::raft_write(
        state,
        crate::state::DesiredStateCommand::UpdateRepositoryMemberRuntime(
            RepositoryMemberRuntimePatch {
                node_id: node_id.as_str().to_owned(),
                update: RepositoryMemberRuntimeUpdate::Capacity {
                    used_bytes: capacity.used_bytes(),
                    filesystem_available_bytes: capacity.filesystem_available_bytes(),
                },
            },
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("write local history repository capacity to Raft"))?;
    Ok(())
}

async fn advance_local_repository_lifecycle(state: &AppState, now: u64) -> anyhow::Result<()> {
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let (lifecycle, ready_repository_ids) = {
        let store = state.store.lock().await;
        let Some(membership) = store.state().repository_membership.as_ref() else {
            return Ok(());
        };
        let Some(member) = membership.repository(&node_id) else {
            return Ok(());
        };
        (
            *member.lifecycle(),
            membership
                .ready_members()
                .map(|member| member.node_id().as_str().to_owned())
                .collect::<Vec<_>>(),
        )
    };
    if lifecycle != RepositoryLifecycle::Syncing {
        return Ok(());
    }

    let caught_up = if ready_repository_ids.is_empty() {
        backfill_initial_repository_from_local_history(state, now).await?
    } else {
        catch_up_against_ready_repositories(state, now).await?
    };
    apply_local_catch_up_result(state, &node_id, now, caught_up).await
}

async fn backfill_initial_repository_from_local_history(
    state: &AppState,
    _now: u64,
) -> anyhow::Result<bool> {
    let local_backfill_completed = state
        .repository_replica
        .lock()
        .await
        .local_history_backfill_completed();
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let identity = super::derived_repository_identity(state, node_id)
        .map_err(|_| anyhow::anyhow!("derive local history backfill identity"))?;
    let signing_key = super::derived_repository_signing_key(state, identity.node_id().as_str())
        .map_err(|_| anyhow::anyhow!("derive local history backfill signing key"))?;
    let mut saw_history = false;
    let ready_repository_ids = vec![state.cluster.node_id.clone()];
    if !local_backfill_completed {
        let mut local_cursor = state
            .repository_replica
            .lock()
            .await
            .local_history_backfill_cursor()
            .map(ToOwned::to_owned);
        loop {
            let collected =
                historical_source_backfill_records(state, local_cursor.as_deref(), 1).await?;
            saw_history |= !collected.records.is_empty();
            let next_cursor = collected.next_cursor()?;
            for (observed_at, records) in historical_record_batches(collected.into_records()) {
                let segments = state
                    .repository_replica
                    .lock()
                    .await
                    .queue_local_source_segments(
                        &state.cluster.cluster_id,
                        identity.clone(),
                        &signing_key,
                        records,
                        observed_at,
                    )?;
                for segment in &segments {
                    receive_local_source_segment(
                        state,
                        segment,
                        &[],
                        &ready_repository_ids,
                        observed_at,
                    )
                    .await?;
                    state
                        .repository_replica
                        .lock()
                        .await
                        .acknowledge_local_source_segment(&segment.wire)?;
                }
            }
            let Some(next_cursor) = next_cursor else {
                state
                    .repository_replica
                    .lock()
                    .await
                    .checkpoint_local_history_backfill(None, true)?;
                break;
            };
            local_cursor = Some(next_cursor);
            state
                .repository_replica
                .lock()
                .await
                .checkpoint_local_history_backfill(local_cursor.clone(), false)?;
        }
        // Peer availability must not replay local pages on every retry. The source outbox has
        // already durably acknowledged every local segment at this point.
    }
    let mut peer_backfill_statuses = Vec::new();
    for peer in all_cluster_peers(state).await {
        let peer_history = pull_peer_initial_history(state, &peer, &ready_repository_ids).await?;
        saw_history |= peer_history.unwrap_or_default();
        peer_backfill_statuses.push(peer_history);
    }
    if !initial_backfill_is_complete(saw_history, &peer_backfill_statuses) {
        // A first repository with no migrated history is deliberately not "caught up". Its
        // lifecycle remains syncing instead of publishing a false complete/local-only window.
        return Ok(false);
    }
    Ok(true)
}

fn initial_backfill_is_complete(
    _saw_history: bool,
    peer_backfill_statuses: &[Option<bool>],
) -> bool {
    // An all-empty cluster is a valid zero-coverage baseline. It is only incomplete when a
    // peer has not responded, because that leaves historic coverage unknown.
    peer_backfill_statuses.iter().all(Option::is_some)
}

async fn pull_peer_initial_history(
    state: &AppState,
    peer: &MeshPeerTarget,
    ready_repository_ids: &[String],
) -> anyhow::Result<Option<bool>> {
    // Imported pre-repository history belongs to the repository that durably observed it. The
    // original node remains the subject and is encoded in the stable key. This lets that
    // repository's signed deletion marker remove peer history without impersonating a peer.
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let identity = super::derived_repository_identity(state, node_id)
        .map_err(|_| anyhow::anyhow!("derive local peer-import identity"))?;
    let signing_key = super::derived_repository_signing_key(state, identity.node_id().as_str())
        .map_err(|_| anyhow::anyhow!("derive local peer-import signing key"))?;
    let epoch = state
        .repository_replica
        .lock()
        .await
        .initial_peer_backfill_epoch(
            &state.cluster.cluster_id,
            identity.node_id().as_str(),
            &peer.node_id,
        )?;
    let checkpoint = state
        .repository_replica
        .lock()
        .await
        .initial_peer_backfill_checkpoint(&peer.node_id)
        .unwrap_or_default();
    let mut cursor = checkpoint.page_cursor;
    let mut stream_state = checkpoint.stream_state;
    let mut saw_history = checkpoint.saw_history;
    if checkpoint.completed {
        return Ok(Some(saw_history));
    }
    loop {
        let page: RepositoryInitialBackfillPage = match repository_direct_request(
            state,
            peer,
            Method::GET,
            &cursor.as_deref().map_or_else(
                || {
                    "/api/admin/_internal/history-repository/initial-backfill?page_size=1"
                        .to_owned()
                },
                |cursor| {
                    format!(
                        "{}?page_cursor={cursor}&page_size=1",
                        "/api/admin/_internal/history-repository/initial-backfill"
                    )
                },
            ),
            Vec::new(),
        )
        .await
        {
            Ok(page) => page,
            Err(error) => {
                tracing::debug!(
                    peer = %peer.node_id,
                    error = %error,
                    "peer history backfill is incomplete"
                );
                return Ok(None);
            }
        };
        if !page.records.is_empty() {
            saw_history = true;
            receive_peer_backfill_page(
                state,
                PeerBackfillImport {
                    identity: &identity,
                    signing_key: &signing_key,
                    peer_node_id: &peer.node_id,
                    epoch,
                    ready_repository_ids,
                },
                page.records,
                &mut stream_state,
            )
            .await?;
        }
        let Some(next_page_cursor) = page.next_page_cursor else {
            state
                .repository_replica
                .lock()
                .await
                .update_initial_peer_backfill_checkpoint(
                    &peer.node_id,
                    None,
                    stream_state,
                    saw_history,
                    true,
                )?;
            return Ok(Some(saw_history));
        };
        if cursor.as_deref() == Some(next_page_cursor.as_str()) {
            anyhow::bail!("peer history backfill page cursor did not advance");
        }
        cursor = Some(next_page_cursor);
        state
            .repository_replica
            .lock()
            .await
            .update_initial_peer_backfill_checkpoint(
                &peer.node_id,
                cursor.clone(),
                stream_state.clone(),
                saw_history,
                false,
            )?;
    }
}

async fn receive_peer_backfill_page(
    state: &AppState,
    import: PeerBackfillImport<'_>,
    records: Vec<RepositoryInitialBackfillRecord>,
    stream_state: &mut BTreeMap<String, (u64, Option<[u8; 32]>)>,
) -> anyhow::Result<()> {
    let mut grouped = BTreeMap::<(u64, String), Vec<SyncRecord>>::new();
    for record in records {
        let (observed_at, record) = record.into_sync_record()?;
        let stream = peer_backfill_stream_for_schema(record.schema().0, import.peer_node_id)?;
        grouped
            .entry((observed_at, stream))
            .or_default()
            .push(record);
    }
    for ((observed_at, stream), records) in grouped {
        for records in records.chunks(32) {
            let (next_sequence, previous_hash) =
                stream_state.entry(stream.clone()).or_insert((0, None));
            let signed = CanonicalSegment::new(
                &state.cluster.cluster_id,
                Cursor::new(
                    import.identity.node_id().as_str(),
                    import.epoch,
                    &stream,
                    *next_sequence,
                )?,
                records.to_vec(),
                *previous_hash,
                observed_at,
                observed_at,
            )?
            .sign(import.signing_key)?;
            let wire = signed.wire_bytes()?;
            *next_sequence = next_sequence.saturating_add(u64::try_from(records.len())?);
            *previous_hash = Some(signed.segment_hash()?);
            state
                .repository_replica
                .lock()
                .await
                .receive_wire_from_repository(
                    &state.cluster.cluster_id,
                    import.identity,
                    &wire,
                    observed_at,
                    import.ready_repository_ids,
                    &state.cluster.node_id,
                )?;
        }
    }
    Ok(())
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

fn peer_backfill_stream_for_schema(schema_id: &str, peer_node_id: &str) -> anyhow::Result<String> {
    let stream = source_stream_for_schema(schema_id)
        .ok_or_else(|| anyhow::anyhow!("peer history has an unsupported source schema"))?;
    Ok(format!("{stream}-backfill-{peer_node_id}"))
}

pub(super) async fn initial_backfill_page(
    state: &AppState,
    page_cursor: Option<&str>,
    page_size: Option<usize>,
) -> anyhow::Result<RepositoryInitialBackfillPage> {
    let page_size = page_size.unwrap_or(MAX_INITIAL_BACKFILL_PAGE_RECORDS);
    if page_size == 0 || page_size > MAX_INITIAL_BACKFILL_PAGE_RECORDS {
        anyhow::bail!("initial history backfill page size exceeds limit");
    }
    let local_is_ready_repository = {
        let store = state.store.lock().await;
        store
            .state()
            .repository_membership
            .as_ref()
            .and_then(|membership| {
                RepositoryNodeId::try_from(state.cluster.node_id.clone())
                    .ok()
                    .and_then(|node_id| membership.repository(&node_id))
            })
            .is_some_and(|member| member.lifecycle() == &RepositoryLifecycle::Ready)
    };
    if local_is_ready_repository {
        let page = state
            .repository_replica
            .lock()
            .await
            .tiered_backfill_page(page_cursor, page_size)?;
        return Ok(RepositoryInitialBackfillPage {
            records: page
                .records
                .into_iter()
                .map(|record| RepositoryInitialBackfillRecord {
                    observed_at_unix_seconds: record.observed_at_unix_seconds,
                    subject_node_id: record.subject_node_id,
                    observer_node_id: record.observer_node_id,
                    schema_id: record.schema_id,
                    schema_version: record.schema_version,
                    record_key_base64: URL_SAFE_NO_PAD.encode(record.record_key),
                    payload_base64: URL_SAFE_NO_PAD.encode(record.payload),
                    tombstone: false,
                })
                .collect(),
            next_page_cursor: page.next_cursor,
        });
    }
    let collected = historical_source_backfill_records(state, page_cursor, page_size).await?;
    let mut page = Vec::new();
    let mut next_cursor = None;
    for (sort_key, (observed_at_unix_seconds, record)) in &collected.records {
        let candidate = RepositoryInitialBackfillRecord {
            observed_at_unix_seconds: *observed_at_unix_seconds,
            subject_node_id: record.subject_node_id().to_owned(),
            observer_node_id: record.observer_node_id().to_owned(),
            schema_id: record.schema().0.to_owned(),
            schema_version: record.schema().1,
            record_key_base64: URL_SAFE_NO_PAD.encode(record.record_key()),
            payload_base64: URL_SAFE_NO_PAD.encode(record.payload_bytes()),
            tombstone: record.is_tombstone(),
        };
        let candidate_bytes = serde_json::to_vec(&candidate)?.len();
        let page_bytes = serde_json::to_vec(&page)?.len();
        if !page.is_empty()
            && page_bytes.saturating_add(candidate_bytes) > MAX_INITIAL_BACKFILL_PAGE_BYTES
        {
            break;
        }
        if candidate_bytes > MAX_INITIAL_BACKFILL_PAGE_BYTES {
            anyhow::bail!("initial history backfill record exceeds page budget");
        }
        page.push(candidate);
        next_cursor = Some(sort_key.clone());
    }
    let next_page_cursor = (collected.has_more || page.len() < collected.records.len())
        .then(|| {
            next_cursor
                .as_ref()
                .expect("nonempty backfill page")
                .encode()
        })
        .transpose()?;
    Ok(RepositoryInitialBackfillPage {
        records: page,
        next_page_cursor,
    })
}

async fn historical_source_backfill_records(
    state: &AppState,
    page_cursor: Option<&str>,
    limit: usize,
) -> anyhow::Result<HistoricalBackfillCollector> {
    let after = page_cursor
        .map(HistoricalBackfillSortKey::decode)
        .transpose()?;
    let mut records = HistoricalBackfillCollector::new(after, limit);
    if let Some(history) = state.node_history.snapshot(&state.cluster.node_id).await {
        let history_node_id = history.node_id.clone();
        for traffic in history.daily_traffic {
            push_backfill_record(
                &mut records,
                "traffic.v1",
                &state.cluster.node_id,
                &format!("{}T00:00:00Z", traffic.date),
                format!(
                    "node-history:node:{history_node_id}:daily-traffic:{}",
                    traffic.date
                ),
                serde_json::to_value(traffic)?,
            )?;
        }
        for status in history.daily_component_status {
            let date = status.date.clone();
            push_backfill_record(
                &mut records,
                "runtime.v1",
                &state.cluster.node_id,
                &format!("{date}T00:00:00Z"),
                format!("node-history:node:{history_node_id}:daily-status:{date}"),
                serde_json::to_value(status)?,
            )?;
        }
        for event in history.component_status_events {
            let occurred_at = event.occurred_at.clone();
            let key = event.event_id.clone();
            push_backfill_record(
                &mut records,
                "path_health.v1",
                &state.cluster.node_id,
                &occurred_at,
                format!("node-history:node:{history_node_id}:event:{key}"),
                serde_json::to_value(event)?,
            )?;
        }
        if let Some(traffic) = history.traffic {
            for bucket in traffic.five_minute {
                let at = bucket.end_at.clone();
                let key = bucket.start_at.clone();
                push_backfill_record(
                    &mut records,
                    "traffic.v1",
                    &state.cluster.node_id,
                    &at,
                    format!("node-history:node:{history_node_id}:five-minute:{key}"),
                    serde_json::to_value(bucket)?,
                )?;
            }
            for bucket in traffic.daily {
                let date = bucket.date.clone();
                push_backfill_record(
                    &mut records,
                    "traffic.v1",
                    &state.cluster.node_id,
                    &format!("{date}T00:00:00Z"),
                    format!("node-history:node:{history_node_id}:daily-rollup:{date}"),
                    serde_json::to_value(bucket)?,
                )?;
            }
        }
    }

    let mesh = state.mesh_telemetry.snapshot().await;
    for peer in mesh.peers {
        let peer_id = peer.peer_id.clone();
        for bucket in peer.buckets {
            let minute = bucket.minute.clone();
            push_backfill_record(
                &mut records,
                "path_health.v1",
                &state.cluster.node_id,
                &minute,
                format!(
                    "node-history:node:{}:mesh:{peer_id}:{minute}",
                    state.cluster.node_id
                ),
                serde_json::to_value(bucket)?,
            )?;
        }
    }

    let (inbound_samples, connection_samples) = {
        let store = state.store.lock().await;
        (
            store
                .inbound_ip_usage()
                .repository_samples_for_node(&state.cluster.node_id),
            store
                .tcp_connection_usage()
                .repository_samples_for_node(&state.cluster.node_id),
        )
    };
    for sample in inbound_samples {
        let minute = sample.minute.clone();
        push_backfill_record(
            &mut records,
            "ip_usage.v1",
            &state.cluster.node_id,
            &minute,
            format!(
                "node-history:node:{}:inbound-ip:{minute}",
                state.cluster.node_id
            ),
            serde_json::to_value(sample)?,
        )?;
    }
    for endpoint in connection_samples {
        let endpoint_id = endpoint.endpoint_id.clone();
        for sample in endpoint.series {
            let minute = sample.minute.clone();
            push_backfill_record(
                &mut records,
                "connections.v1",
                &state.cluster.node_id,
                &minute,
                format!(
                    "node-history:node:{}:tcp:{endpoint_id}:{minute}",
                    state.cluster.node_id
                ),
                serde_json::to_value(sample)?,
            )?;
        }
    }
    Ok(records)
}

fn push_backfill_record(
    records: &mut HistoricalBackfillCollector,
    schema_id: &str,
    node_id: &str,
    observed_at: &str,
    record_key: String,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    let observed_at = chrono::DateTime::parse_from_rfc3339(observed_at)
        .map_err(|error| {
            anyhow::anyhow!("parse persisted history timestamp {observed_at}: {error}")
        })?
        .timestamp()
        .try_into()
        .map_err(|_| anyhow::anyhow!("persisted history timestamp is before Unix epoch"))?;
    records.push((
        observed_at,
        source_record_with_key(
            schema_id,
            node_id,
            observed_at,
            record_key.into_bytes(),
            payload,
            false,
        )?,
    ));
    Ok(())
}

fn historical_record_batches(records: Vec<(u64, SyncRecord)>) -> Vec<(u64, Vec<SyncRecord>)> {
    const MAX_RECORDS_PER_BACKFILL_SEGMENT: usize = 32;
    let mut batches = Vec::new();
    let mut current_timestamp = None;
    let mut current_records = Vec::new();
    for (observed_at, record) in records {
        if current_timestamp != Some(observed_at)
            || current_records.len() == MAX_RECORDS_PER_BACKFILL_SEGMENT
        {
            if let Some(timestamp) = current_timestamp {
                batches.push((timestamp, std::mem::take(&mut current_records)));
            }
            current_timestamp = Some(observed_at);
        }
        current_records.push(record);
    }
    if let Some(timestamp) = current_timestamp {
        batches.push((timestamp, current_records));
    }
    batches
}

async fn catch_up_against_ready_repositories(state: &AppState, now: u64) -> anyhow::Result<bool> {
    let (ready_repository_ids, peers) = ready_repository_peers(state).await?;
    if peers.len() != ready_repository_ids.len() {
        return Ok(false);
    }
    let mut receiving_repository_ids = ready_repository_ids.clone();
    receiving_repository_ids.push(state.cluster.node_id.clone());
    receiving_repository_ids.sort_unstable();
    receiving_repository_ids.dedup();
    {
        let mut runtime = state.repository_replica.lock().await;
        runtime.prepare_for_replication(now)?;
        runtime.reconcile_ready_repositories(&receiving_repository_ids)?;
    }

    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
    {
        match replicate_peer(
            state,
            peer,
            &receiving_repository_ids,
            now,
            ReplicaWork::DeepVerification,
            false,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => return Ok(false),
            Err(error) => {
                tracing::debug!(
                    peer = %peer.node_id,
                    error = %error,
                    "history repository catch-up verification failed"
                );
                return Ok(false);
            }
        }
    }
    // Signed anti-entropy frames are deliberately short-lived. Before this member can become
    // ready, reconstruct the older tiered window from every node's migrated local history over
    // the same authenticated Mesh/Tunnel direct paths. Checkpoints make this bounded import
    // resumable across transient peer failures.
    for peer in all_cluster_peers(state).await {
        if pull_peer_initial_history(state, &peer, &receiving_repository_ids)
            .await?
            .is_none()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn apply_local_catch_up_result(
    state: &AppState,
    node_id: &RepositoryNodeId,
    now: u64,
    caught_up: bool,
) -> anyhow::Result<()> {
    let update = {
        let store = state.store.lock().await;
        let Some(member) = store
            .state()
            .repository_membership
            .as_ref()
            .and_then(|membership| membership.repository(node_id))
        else {
            return Ok(());
        };
        if member.lifecycle() != &RepositoryLifecycle::Syncing {
            return Ok(());
        }
        if caught_up {
            match member.catch_up_completed_at() {
                None => Some(RepositoryMemberRuntimeUpdate::CatchUpComplete { completed_at: now }),
                Some(completed_at)
                    if now.saturating_sub(completed_at) >= READY_STABILITY_WINDOW.as_secs() =>
                {
                    Some(RepositoryMemberRuntimeUpdate::Ready { ready_at: now })
                }
                Some(_) => None,
            }
        } else {
            Some(RepositoryMemberRuntimeUpdate::CatchUpIncomplete)
        }
    };
    let Some(update) = update else {
        return Ok(());
    };
    super::super::raft_write(
        state,
        crate::state::DesiredStateCommand::UpdateRepositoryMemberRuntime(
            RepositoryMemberRuntimePatch {
                node_id: node_id.as_str().to_owned(),
                update,
            },
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("write local history repository lifecycle to Raft"))?;
    Ok(())
}

async fn update_local_replica_convergence(
    state: &AppState,
    replica_converged: bool,
) -> anyhow::Result<()> {
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let update_needed = {
        let store = state.store.lock().await;
        store
            .state()
            .repository_membership
            .as_ref()
            .and_then(|membership| membership.repository(&node_id))
            .is_some_and(|member| {
                member.lifecycle() == &RepositoryLifecycle::Ready
                    && member.replica_converged() != replica_converged
            })
    };
    if !update_needed {
        return Ok(());
    }
    super::super::raft_write(
        state,
        crate::state::DesiredStateCommand::UpdateRepositoryMemberRuntime(
            RepositoryMemberRuntimePatch {
                node_id: node_id.as_str().to_owned(),
                update: RepositoryMemberRuntimeUpdate::ReplicaConverged { replica_converged },
            },
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("write local history repository convergence to Raft"))?;
    Ok(())
}

async fn known_history_source_node_ids(state: &AppState) -> Vec<String> {
    const MAX_KNOWN_HISTORY_SOURCES: usize = 4_096;

    let store = state.store.lock().await;
    store
        .state()
        .nodes
        .keys()
        .take(MAX_KNOWN_HISTORY_SOURCES)
        .cloned()
        .collect()
}

fn completed_replication_work(work: ReplicaWork, deep_verification_succeeded: bool) -> ReplicaWork {
    if work.is_deep_verification() && !deep_verification_succeeded {
        ReplicaWork::AntiEntropy
    } else {
        work
    }
}

async fn replicate_peer_via_dynamic_relay(
    state: &AppState,
    target: &MeshPeerTarget,
    _peers: &[MeshPeerTarget],
    _ready_repository_ids: &[String],
    now: u64,
) -> anyhow::Result<()> {
    let relay_peers = eligible_mesh_relay_peers(state).await;
    let relay = relay_peers
        .iter()
        .find(|peer| peer.node_id != state.cluster.node_id && peer.node_id != target.node_id)
        .ok_or_else(|| anyhow::anyhow!("no independent ready repository can relay"))?;
    let keypair = cluster_relay_keypair(state, &state.cluster.node_id)?;
    let target_public_key = cluster_relay_keypair(state, &target.node_id)?.public_key();
    let page = {
        let mut runtime = state.repository_replica.lock().await;
        if !runtime.begin_dynamic_relay_attempt(now)? {
            anyhow::bail!("hourly jittered dynamic relay attempt is not due");
        }
        runtime.relay_batch(&target.node_id)?
    };
    let next_segment_id = page.next_segment_id().map(str::to_owned);
    if page.batch.segments.is_empty() && page.batch.gaps.is_empty() {
        return Ok(());
    }
    let frame = RelayFrame::seal(
        keypair,
        target_public_key,
        rand::random(),
        &page.payload,
        target.node_id.as_bytes(),
    )?;
    let body = serde_json::to_vec(&RepositoryRelayRequest {
        target_repository_id: target.node_id.clone(),
        source_repository_id: state.cluster.node_id.clone(),
        relay_repository_id: None,
        frame,
    })?;
    repository_mesh_request::<serde_json::Value>(
        state,
        relay,
        Method::POST,
        INTERNAL_HISTORY_REPOSITORY_RELAY,
        body,
    )
    .await?;
    state
        .repository_replica
        .lock()
        .await
        .record_relay_batch_delivered(&target.node_id, next_segment_id.as_deref())?;
    Ok(())
}

pub(super) fn cluster_relay_keypair(
    state: &AppState,
    node_id: &str,
) -> anyhow::Result<RelayKeypair> {
    if node_id.is_empty() {
        anyhow::bail!("relay node id is empty");
    }
    let cluster_ca_key = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cluster CA key is not available"))?;
    Ok(relay_keypair_from_cluster_material(
        &state.cluster.cluster_id,
        node_id,
        cluster_ca_key,
    ))
}

fn relay_keypair_from_cluster_material(
    cluster_id: &str,
    node_id: &str,
    cluster_ca_key: &str,
) -> RelayKeypair {
    let mut hasher = Sha256::new();
    hasher.update(CLUSTER_RELAY_KEY_CONTEXT);
    hasher.update(cluster_id.as_bytes());
    hasher.update([0]);
    hasher.update(node_id.as_bytes());
    hasher.update([0]);
    hasher.update(cluster_ca_key.as_bytes());
    RelayKeypair::from_private_key(hasher.finalize().into())
}

async fn replicate_peer(
    state: &AppState,
    peer: &MeshPeerTarget,
    ready_repository_ids: &[String],
    now: u64,
    work: ReplicaWork,
    propagate_acknowledgements: bool,
) -> anyhow::Result<bool> {
    let mut after_segment_id = None::<String>;
    let mut repaired = false;
    loop {
        let path = after_segment_id.as_ref().map_or_else(
            || "/api/admin/_internal/history-repository/summary".to_owned(),
            |cursor| {
                format!("/api/admin/_internal/history-repository/summary?after_segment_id={cursor}")
            },
        );
        let remote_summary: RepositoryReplicaSummary =
            repository_direct_request(state, peer, Method::GET, &path, Vec::new()).await?;
        let (requires_repair, missing_segment_ids) = {
            let runtime = state.repository_replica.lock().await;
            (
                runtime.requires_repair(&remote_summary, work.is_deep_verification())?,
                runtime.missing_segment_ids(&remote_summary, work.is_deep_verification())?,
            )
        };
        if requires_repair {
            let repair_body = serde_json::to_vec(&RepositoryRepairRequest {
                segment_ids: missing_segment_ids,
            })?;
            let repair: crate::state::history_repository::replica::RepositoryRepairBatch =
                repository_direct_request(
                    state,
                    peer,
                    Method::POST,
                    "/api/admin/_internal/history-repository/repair",
                    repair_body,
                )
                .await?;
            let mut acknowledgements = Vec::new();
            for segment in repair.segments {
                if !super::identity_is_pinned_for_node(state, &segment.identity)
                    .await
                    .map_err(|_| anyhow::anyhow!("check repository repair segment identity"))?
                {
                    anyhow::bail!("repository repair segment identity is not pinned")
                }
                let receipt = state
                    .repository_replica
                    .lock()
                    .await
                    .receive_wire_from_repository(
                        &state.cluster.cluster_id,
                        &segment.identity,
                        &segment.wire,
                        now,
                        ready_repository_ids,
                        &state.cluster.node_id,
                    )?;
                acknowledgements.extend(receipt.tombstone_acknowledgements().iter().cloned());
            }
            state
                .repository_replica
                .lock()
                .await
                .merge_replica_gaps(&repair.gaps)?;
            if propagate_acknowledgements {
                propagate_tombstone_acknowledgements(state, ready_repository_ids, acknowledgements)
                    .await?;
            }
            repaired = true;
        }
        let Some(next) = remote_summary.next_segment_id else {
            break;
        };
        if after_segment_id.as_deref() == Some(next.as_str()) {
            anyhow::bail!("repository summary cursor did not advance")
        }
        after_segment_id = Some(next);
    }
    // A repair is a one-way pull. Require a later direct summary exchange before
    // treating this peer as equal, because it may still need our own segments.
    Ok(!repaired)
}

pub(super) async fn propagate_tombstone_acknowledgements(
    state: &AppState,
    _ready_repository_ids: &[String],
    acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
) -> anyhow::Result<()> {
    if acknowledgements.is_empty() {
        return Ok(());
    }
    let peers = all_cluster_peers(state).await;
    let body = serde_json::to_vec(&RepositoryTombstoneAcknowledgementRequest { acknowledgements })?;
    let mut first_delivery_error = None::<anyhow::Error>;
    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
    {
        if let Err(error) = repository_direct_request::<serde_json::Value>(
            state,
            peer,
            Method::POST,
            "/api/admin/_internal/history-repository/tombstone-ack",
            body.clone(),
        )
        .await
        {
            first_delivery_error.get_or_insert(error.into());
        }
    }
    first_delivery_error.map_or(Ok(()), Err)
}

pub(super) async fn ready_repository_peers(
    state: &AppState,
) -> anyhow::Result<(Vec<String>, Vec<MeshPeerTarget>)> {
    let store = state.store.lock().await;
    let membership = store
        .state()
        .repository_membership
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("history repository membership is not configured"))?;
    let ready_repository_ids = membership
        .ready_members()
        .map(|member| member.node_id().as_str().to_owned())
        .collect::<Vec<_>>();
    if ready_repository_ids.is_empty() {
        anyhow::bail!("no ready history repository is available");
    }
    let endpoints = store.list_endpoints();
    let peers = ready_repository_ids
        .iter()
        .filter_map(|repository_id| store.get_node(repository_id))
        .map(|node| peer_target_from_node(&node, &endpoints))
        .collect();
    Ok((ready_repository_ids, peers))
}

#[cfg(test)]
mod tests {
    use super::{
        RelayFrame, ReplicaWork, completed_replication_work, relay_keypair_from_cluster_material,
        source_record_with_key, source_records_with_deletions,
    };

    #[test]
    fn dynamic_relay_keys_are_deterministic_per_cluster_and_recipient() {
        let sender = relay_keypair_from_cluster_material("cluster-a", "repository-a", "ca-key");
        let recipient = relay_keypair_from_cluster_material("cluster-a", "repository-b", "ca-key");
        let frame = RelayFrame::seal(
            sender,
            recipient.public_key(),
            [3; 12],
            b"repair batch",
            b"repository-b",
        )
        .expect("seal relay frame");
        assert_eq!(
            frame
                .open(
                    relay_keypair_from_cluster_material("cluster-a", "repository-b", "ca-key"),
                    b"repository-b",
                )
                .expect("open relay frame"),
            b"repair batch"
        );
        assert_ne!(
            recipient.public_key(),
            relay_keypair_from_cluster_material("cluster-a", "repository-c", "ca-key").public_key()
        );
    }

    #[test]
    fn relay_repair_does_not_complete_daily_deep_verification() {
        assert_eq!(
            completed_replication_work(ReplicaWork::DeepVerification, false),
            ReplicaWork::AntiEntropy
        );
        assert_eq!(
            completed_replication_work(ReplicaWork::DeepVerification, true),
            ReplicaWork::DeepVerification
        );
    }

    #[test]
    fn source_deletion_producer_queues_the_independent_tombstone_before_matching_history() {
        let historical_key = b"node-history:node:node-a:daily-traffic:2026-08-14".to_vec();
        let records = source_records_with_deletions(
            "node-a",
            100,
            vec![(
                "traffic.v1".to_owned(),
                b"node-history:node:node-a:".to_vec(),
                Some("node-a".to_owned()),
            )],
            vec![
                source_record_with_key(
                    "traffic.v1",
                    "node-a",
                    100,
                    historical_key.clone(),
                    serde_json::json!({ "sample": true }),
                    false,
                )
                .expect("historical source record"),
            ],
        )
        .expect("production source records");

        assert!(records[0].is_tombstone());
        assert_eq!(records[0].schema().0, "traffic.v1");
        assert_eq!(records[0].record_key(), b"node-history:node:node-a:");
        assert!(!records[1].is_tombstone());
        assert_eq!(records[1].record_key(), historical_key);
    }

    #[test]
    fn peer_transport_failure_keeps_the_first_repository_syncing() {
        assert!(!super::initial_backfill_is_complete(
            true,
            &[Some(true), None]
        ));
        assert!(super::initial_backfill_is_complete(false, &[Some(false)]));
        assert!(super::initial_backfill_is_complete(
            true,
            &[Some(false), Some(true)]
        ));
    }

    #[test]
    fn peer_backfill_streams_are_independent_from_the_live_source_cursor_chain() {
        assert_eq!(
            super::peer_backfill_stream_for_schema("traffic.v1", "node-a")
                .expect("backfill stream"),
            "traffic-backfill-node-a"
        );
        assert_ne!(
            super::peer_backfill_stream_for_schema("traffic.v1", "node-a")
                .expect("backfill stream"),
            super::source_stream_for_schema("traffic.v1").expect("live stream")
        );
    }

    #[test]
    fn initial_history_backfill_collector_keeps_only_one_bounded_page() {
        let mut collector = super::HistoricalBackfillCollector::new(None, 64);
        for sequence in 0..130_u64 {
            collector.push((
                sequence,
                source_record_with_key(
                    "runtime.v1",
                    "node-a",
                    sequence,
                    format!("node-history:node:node-a:{sequence}").into_bytes(),
                    serde_json::json!({ "sequence": sequence }),
                    false,
                )
                .expect("bounded record"),
            ));
        }
        assert_eq!(collector.records.len(), 64);
        assert!(collector.has_more);
        assert_eq!(
            collector
                .records
                .first_key_value()
                .expect("first record")
                .1
                .0,
            0
        );
        assert_eq!(
            collector.records.last_key_value().expect("last record").1.0,
            63
        );
        let cursor = collector
            .next_cursor()
            .expect("cursor encoding")
            .expect("more history");
        let mut next = super::HistoricalBackfillCollector::new(
            Some(super::HistoricalBackfillSortKey::decode(&cursor).expect("cursor decoding")),
            64,
        );
        for sequence in 0..130_u64 {
            next.push((
                sequence,
                source_record_with_key(
                    "runtime.v1",
                    "node-a",
                    sequence,
                    format!("node-history:node:node-a:{sequence}").into_bytes(),
                    serde_json::json!({ "sequence": sequence }),
                    false,
                )
                .expect("bounded record"),
            ));
        }
        assert_eq!(next.records.len(), 64);
        assert!(next.has_more);
        assert_eq!(
            next.records.first_key_value().expect("first record").1.0,
            64
        );
        assert_eq!(next.records.last_key_value().expect("last record").1.0, 127);
    }
}
