use super::super::AppState;
use super::{
    INTERNAL_HISTORY_REPOSITORY_RELAY, RepositoryRelayRequest, RepositoryRepairRequest,
    RepositorySyncRequest, RepositoryTombstoneAcknowledgementRequest,
};
use crate::{
    control_plane_mesh::MeshPeerTarget,
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
use axum::http::Method;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::time::MissedTickBehavior;
const REPOSITORY_REPLICATION_INTERVAL: Duration = Duration::from_secs(5 * 60);
const SOURCE_COLLECTION_INTERVAL: Duration = Duration::from_secs(60);
const REPOSITORY_REQUEST_BUDGET: Duration = Duration::from_secs(15);
const MAX_REPOSITORY_PEERS_PER_CYCLE: usize = 4;
const READY_STABILITY_WINDOW: Duration = Duration::from_secs(5 * 60);
const MAX_SOURCE_PAYLOAD_BYTES: usize = 32 * 1024;
const MAX_SOURCE_SUMMARY_ITEMS: usize = 64;
const CLUSTER_RELAY_KEY_CONTEXT: &[u8] = b"xp-history-repository-relay-key-v1\0";
mod backfill;
mod deep_repair;
mod direct;
mod legacy_segment_index;
mod ready_peers;
mod source;
mod source_records;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) use backfill::{
    HistoricalBackfillCollector, HistoricalBackfillSortKey, peer_backfill_stream_for_record,
    peer_backfill_stream_for_schema, should_restart_peer_backfill, source_stream_for_schema,
};
pub(super) use backfill::{RepositoryInitialBackfillPage, initial_backfill_page};
use backfill::{
    backfill_initial_repository_from_local_history, catch_up_against_ready_repositories,
    pull_peer_initial_history,
};
#[cfg(test)]
use deep_repair::deep_repair_requires_tiered_backfill;
use deep_repair::restart_tiered_backfill_after_incomplete_deep_repair;
pub(super) use direct::{
    RepositoryDirectError, all_cluster_peers, eligible_mesh_relay_peers, is_transport_failure,
    repository_direct_request, repository_mesh_request,
};
pub(super) use ready_peers::ready_repository_peers;
use source::{
    local_repository_lifecycle, repair_legacy_tombstone_metadata, should_attempt_source_relay,
    should_fanout_tombstone_acknowledgements, source_record, source_record_with_key,
    source_record_with_key_for_subject,
};
use source_records::{source_records, source_records_with_deletions};

pub(crate) fn spawn_repository_replica_worker(state: AppState) {
    legacy_segment_index::spawn(state.clone());
    let source_state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SOURCE_COLLECTION_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
            if let Err(error) = sync_local_repository_capacity(&source_state, now).await {
                tracing::debug!(error = %error, "history repository capacity cycle skipped");
            }
            if let Err(error) = advance_local_repository_lifecycle(&source_state, now).await {
                tracing::debug!(error = %error, "history repository lifecycle cycle skipped");
            }
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
            identity.clone(),
            &signing_key,
            source_batch.records,
            now,
            ready_repository_ids,
        )?;
        let gaps = runtime.local_source_backpressure_gaps(&state.cluster.node_id);
        (segments, gaps)
    };
    if segments.is_empty() && gaps.is_empty() {
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
    if segments.is_empty() {
        (delivery_succeeded, transport_failed) =
            super::gaps::deliver_source_gaps(state, selected_peer, identity.clone(), gaps.clone())
                .await?;
    }
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
                    .acknowledge_local_source_segment_via(&segment.wire, now, "local")?;
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
                        .acknowledge_local_source_segment_via(&segment.wire, now, "direct")?;
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
    if payload.batch.segments.is_empty() && payload.batch.gaps.is_empty() {
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
            .acknowledge_local_source_segment_via(&segment.wire, now, "dynamic_relay")?;
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
    if should_fanout_tombstone_acknowledgements(local_repository_lifecycle(state).await?)
        && !receipt.tombstone_acknowledgements().is_empty()
    {
        propagate_tombstone_acknowledgements(
            state,
            ready_repository_ids,
            receipt.tombstone_acknowledgements().to_vec(),
        )
        .await?;
    }
    Ok(())
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
    repair_legacy_tombstone_metadata(state, now).await?;
    let node_id = RepositoryNodeId::try_from(state.cluster.node_id.clone())?;
    let (lifecycle, catch_up_completed, ready_repository_ids) = {
        let store = state.store.lock().await;
        let Some(membership) = store.state().repository_membership.as_ref() else {
            return Ok(());
        };
        let Some(member) = membership.repository(&node_id) else {
            return Ok(());
        };
        (
            *member.lifecycle(),
            member.catch_up_completed_at().is_some(),
            membership
                .ready_members()
                .map(|member| member.node_id().as_str().to_owned())
                .collect::<Vec<_>>(),
        )
    };
    if lifecycle != RepositoryLifecycle::Syncing {
        return Ok(());
    }

    // Catch-up validates a bounded point in time. Once it completes, continuously arriving
    // source segments must not reset the five-minute stability window: a busy cluster otherwise
    // has no instant at which it can be exactly equal to an actively writing repository.
    let catch_up = if !should_run_initial_catch_up(catch_up_completed) {
        backfill::InitialBackfillProgress::Complete
    } else if ready_repository_ids.is_empty() {
        backfill_initial_repository_from_local_history(state, now).await?
    } else {
        catch_up_against_ready_repositories(state, now).await?
    };
    match catch_up {
        backfill::InitialBackfillProgress::InProgress => Ok(()),
        backfill::InitialBackfillProgress::Complete => {
            apply_local_catch_up_result(state, &node_id, now, true).await
        }
        backfill::InitialBackfillProgress::Unavailable => {
            apply_local_catch_up_result(state, &node_id, now, false).await
        }
    }
}

fn should_run_initial_catch_up(catch_up_completed: bool) -> bool {
    !catch_up_completed
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

fn remove_delivered_repair_segment_ids<'a>(
    pending_segment_ids: &mut BTreeSet<String>,
    delivered_wires: impl IntoIterator<Item = &'a [u8]>,
) -> anyhow::Result<()> {
    let delivered_ids = delivered_wires
        .into_iter()
        .map(|wire| hex::encode(Sha256::digest(wire)))
        .collect::<Vec<_>>();
    let delivered_set = delivered_ids.iter().cloned().collect::<BTreeSet<_>>();
    if delivered_set.len() != delivered_ids.len() {
        anyhow::bail!("repository repair response repeated a segment")
    }
    if delivered_set.is_empty() || !delivered_set.is_subset(pending_segment_ids) {
        anyhow::bail!("repository repair response did not advance the requested segment set")
    }
    pending_segment_ids.retain(|segment_id| !delivered_set.contains(segment_id));
    Ok(())
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
    loop {
        let path = after_segment_id.as_ref().map_or_else(
            || {
                format!(
                    "/api/admin/_internal/history-repository/summary?deep_verification={}",
                    work.is_deep_verification()
                )
            },
            |cursor| {
                format!("/api/admin/_internal/history-repository/summary?after_segment_id={cursor}")
            },
        );
        let remote_summary: RepositoryReplicaSummary =
            repository_direct_request(state, peer, Method::GET, &path, Vec::new()).await?;
        let requires_repair = {
            let runtime = state.repository_replica.lock().await;
            runtime.requires_repair(&remote_summary, work.is_deep_verification())?
        };
        if requires_repair {
            let mut acknowledgements = Vec::new();
            let mut needs_gap_refresh = true;
            loop {
                let mut pending_segment_ids = {
                    let runtime = state.repository_replica.lock().await;
                    runtime
                        .missing_segment_ids(&remote_summary, work.is_deep_verification())?
                        .into_iter()
                        .collect::<BTreeSet<_>>()
                };
                if pending_segment_ids.is_empty() && !needs_gap_refresh {
                    break;
                }
                while needs_gap_refresh || !pending_segment_ids.is_empty() {
                    needs_gap_refresh = false;
                    let repair_body = serde_json::to_vec(&RepositoryRepairRequest {
                        segment_ids: pending_segment_ids.iter().cloned().collect(),
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
                    if pending_segment_ids.is_empty() {
                        if !repair.segments.is_empty() {
                            anyhow::bail!(
                                "repository repair response returned unrequested segments"
                            )
                        }
                    } else {
                        remove_delivered_repair_segment_ids(
                            &mut pending_segment_ids,
                            repair
                                .segments
                                .iter()
                                .map(|segment| segment.wire.as_slice()),
                        )?;
                    }
                    for segment in repair.segments {
                        if !super::identity_is_pinned_for_node(state, &segment.identity)
                            .await
                            .map_err(|_| {
                                anyhow::anyhow!("check repository repair segment identity")
                            })?
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
                        acknowledgements
                            .extend(receipt.tombstone_acknowledgements().iter().cloned());
                    }
                    state
                        .repository_replica
                        .lock()
                        .await
                        .merge_replica_gaps(&repair.gaps)?;
                }
            }
            if propagate_acknowledgements {
                propagate_tombstone_acknowledgements(state, ready_repository_ids, acknowledgements)
                    .await?;
            }
            let (remaining_segment_repairs, repair_remains_after_segment_repairs) = {
                let runtime = state.repository_replica.lock().await;
                (
                    !runtime
                        .missing_segment_ids(&remote_summary, work.is_deep_verification())?
                        .is_empty(),
                    runtime.requires_repair(&remote_summary, work.is_deep_verification())?,
                )
            };
            if remaining_segment_repairs || repair_remains_after_segment_repairs {
                let restarted_tiered_backfill = {
                    let mut runtime = state.repository_replica.lock().await;
                    restart_tiered_backfill_after_incomplete_deep_repair(
                        &mut runtime,
                        &peer.node_id,
                        work,
                        remaining_segment_repairs,
                        repair_remains_after_segment_repairs,
                    )?
                };
                if restarted_tiered_backfill {
                    pull_peer_initial_history(state, peer, ready_repository_ids).await?;
                }
                return Ok(false);
            }
        }
        let Some(next) = remote_summary.next_segment_id else {
            break;
        };
        if after_segment_id.as_deref() == Some(next.as_str()) {
            anyhow::bail!("repository summary cursor did not advance")
        }
        after_segment_id = Some(next);
    }
    // The keyset traversal defines a bounded remote snapshot. Once every advertised segment
    // and gap has been applied, this replica is caught up to that snapshot. Re-querying a live
    // source for exact equality is not a valid convergence condition: ordinary sources append
    // continuously, while the peer independently performs the symmetric pull.
    Ok(true)
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
