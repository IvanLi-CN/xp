use axum::http::Method;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest as _, Sha256};
use std::time::Duration;
use tokio::time::MissedTickBehavior;

use crate::{
    control_plane_mesh::{MeshPeerTarget, peer_target_from_node},
    history_sync::{RelayFrame, RelayKeypair, SyncRecord},
    state::history_repository::{
        control::{
            RepositoryLifecycle, RepositoryMemberRuntimePatch, RepositoryMemberRuntimeUpdate,
        },
        identity::RepositoryNodeId,
        replica::{
            ReplicaWork, RepositoryRepairBatch, RepositoryReplicaSegment, RepositoryReplicaSummary,
            RepositoryTombstoneAcknowledgement, rendezvous_collectors,
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
const CLUSTER_RELAY_KEY_CONTEXT: &[u8] = b"xp-history-repository-relay-key-v1\0";
mod direct;
mod source;
pub(super) use direct::{all_cluster_peers, is_transport_failure, repository_direct_request};
use source::{should_attempt_source_relay, source_record};

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
    let records = source_records(state, now).await?;
    let (segments, gaps) = {
        let mut runtime = state.repository_replica.lock().await;
        let segments = runtime.queue_local_source_segments(
            &state.cluster.cluster_id,
            identity,
            &signing_key,
            records,
            now,
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
            let body = serde_json::to_vec(&RepositorySyncRequest {
                identity: segment.identity.clone(),
                wire_base64: URL_SAFE_NO_PAD.encode(&segment.wire),
                gaps: gaps.clone(),
            })?;
            match repository_direct_request::<serde_json::Value>(
                state,
                selected_peer,
                Method::POST,
                "/api/admin/_internal/history-repository/sync",
                body,
            )
            .await
            {
                Ok(_) => {
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
    if should_attempt_source_relay(
        transport_failed,
        selected_repository_id == state.cluster.node_id,
    ) {
        let relay_peers = all_cluster_peers(state).await;
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
    repository_direct_request::<serde_json::Value>(
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

async fn source_records(state: &AppState, now: u64) -> anyhow::Result<Vec<SyncRecord>> {
    let runtime = state.node_runtime.snapshot(50).await;
    let history = state.node_history.snapshot(&state.cluster.node_id).await;
    let mesh = state.mesh_telemetry.snapshot().await;
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
    [
        source_record(
            "runtime.v1",
            &state.cluster.node_id,
            now,
            serde_json::to_value(runtime)?,
        ),
        source_record(
            "traffic.v1",
            &state.cluster.node_id,
            now,
            traffic.unwrap_or(serde_json::Value::Null),
        ),
        source_record("path_health.v1", &state.cluster.node_id, now, path_health),
        source_record("ip_usage.v1", &state.cluster.node_id, now, inbound_ip),
        source_record("connections.v1", &state.cluster.node_id, now, connections),
    ]
    .into_iter()
    .collect()
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
        // The first repository has no predecessor to repair from. Its empty state is
        // a complete bootstrap snapshot and must still remain stable before readiness.
        true
    } else {
        catch_up_against_ready_repositories(state, now).await?
    };
    apply_local_catch_up_result(state, &node_id, now, caught_up).await
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
    peers: &[MeshPeerTarget],
    _ready_repository_ids: &[String],
    now: u64,
) -> anyhow::Result<()> {
    let relay = peers
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
    repository_direct_request::<serde_json::Value>(
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
    let remote_summary: RepositoryReplicaSummary = repository_direct_request(
        state,
        peer,
        Method::GET,
        "/api/admin/_internal/history-repository/summary",
        Vec::new(),
    )
    .await?;
    let (requires_repair, missing_segment_ids) = {
        let runtime = state.repository_replica.lock().await;
        (
            runtime.requires_repair(&remote_summary, work.is_deep_verification())?,
            runtime.missing_segment_ids(&remote_summary, work.is_deep_verification())?,
        )
    };
    if !requires_repair {
        return Ok(true);
    }
    let repair = RepositoryRepairRequest {
        segment_ids: missing_segment_ids,
    };
    let repair_body = serde_json::to_vec(&repair)?;
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
        propagate_tombstone_acknowledgements(state, ready_repository_ids, acknowledgements).await?;
    }
    // A repair is a one-way pull. Require a later direct summary exchange before
    // treating this peer as equal, because it may still need our own segments.
    Ok(false)
}

pub(super) async fn propagate_tombstone_acknowledgements(
    state: &AppState,
    ready_repository_ids: &[String],
    acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
) -> anyhow::Result<()> {
    if acknowledgements.is_empty() {
        return Ok(());
    }
    let (_, peers) = ready_repository_peers(state).await?;
    let body = serde_json::to_vec(&RepositoryTombstoneAcknowledgementRequest { acknowledgements })?;
    let mut first_delivery_error = None::<anyhow::Error>;
    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
        .filter(|peer| ready_repository_ids.iter().any(|id| id == &peer.node_id))
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
}
