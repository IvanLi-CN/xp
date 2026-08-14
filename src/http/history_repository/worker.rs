use std::time::Duration;

use axum::http::Method;
use serde::de::DeserializeOwned;
use tokio::time::MissedTickBehavior;

use crate::{
    control_plane_mesh::{MeshPeerTarget, MeshRequest, PeerDirectPath, peer_target_from_node},
    history_sync::RelayFrame,
    internal_auth::InternalRoute,
    state::history_repository::identity::RepositoryNodeId,
    state::history_repository::replica::{
        RepositoryReplicaSummary, RepositoryTombstoneAcknowledgement,
    },
};

use super::super::AppState;
use super::{
    RepositoryRelayRequest, RepositoryRepairRequest, RepositoryTombstoneAcknowledgementRequest,
};

const REPOSITORY_REPLICATION_INTERVAL: Duration = Duration::from_secs(5 * 60);
const REPOSITORY_REQUEST_BUDGET: Duration = Duration::from_secs(15);
const MAX_REPOSITORY_PEERS_PER_CYCLE: usize = 4;

pub(crate) fn spawn_repository_replica_worker(state: AppState) {
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
    let (ready_repository_ids, peers) = ready_repository_peers(state).await?;
    if !ready_repository_ids
        .iter()
        .any(|id| id == &state.cluster.node_id)
    {
        return Ok(());
    }

    let work = {
        let mut runtime = state.repository_replica.lock().await;
        runtime.prepare_for_replication(now)?;
        runtime.reconcile_ready_repositories(&ready_repository_ids)?;
        runtime.replication_work(now)
    };
    if !work.is_anti_entropy() {
        return Ok(());
    }

    let mut synchronized = false;
    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
        .take(MAX_REPOSITORY_PEERS_PER_CYCLE)
    {
        match replicate_peer(state, peer, &ready_repository_ids, now).await {
            Ok(()) => synchronized = true,
            Err(error) => {
                tracing::debug!(
                    peer = %peer.node_id,
                    error = %error,
                    "history repository peer replication failed"
                );
                if let Err(relay_error) = replicate_peer_via_dynamic_relay(
                    state,
                    peer,
                    &peers,
                    &ready_repository_ids,
                    now,
                )
                .await
                {
                    tracing::debug!(
                        peer = %peer.node_id,
                        error = %relay_error,
                        "history repository dynamic relay was unavailable"
                    );
                } else {
                    synchronized = true;
                }
            }
        }
    }
    if synchronized || peers.len() == 1 {
        state
            .repository_replica
            .lock()
            .await
            .record_replication_completed(now, work)?;
    }
    Ok(())
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
    let target_public_key = ready_relay_public_key(state, &target.node_id).await?;
    let (keypair, batch) = {
        let mut runtime = state.repository_replica.lock().await;
        if !runtime.begin_dynamic_relay_attempt(now)? {
            anyhow::bail!("hourly jittered dynamic relay attempt is not due");
        }
        (runtime.relay_keypair()?, runtime.relay_batch())
    };
    if batch.segments.is_empty() {
        return Ok(());
    }
    let payload = serde_json::to_vec(&batch)?;
    let frame = RelayFrame::seal(
        keypair,
        target_public_key,
        rand::random(),
        &payload,
        target.node_id.as_bytes(),
    )?;
    let body = serde_json::to_vec(&RepositoryRelayRequest {
        target_repository_id: target.node_id.clone(),
        source_repository_id: state.cluster.node_id.clone(),
        frame,
    })?;
    repository_direct_request::<serde_json::Value>(
        state,
        relay,
        Method::POST,
        "/_internal/history-repository/relay",
        body,
    )
    .await?;
    Ok(())
}

async fn ready_relay_public_key(state: &AppState, node_id: &str) -> anyhow::Result<[u8; 32]> {
    let node_id = RepositoryNodeId::try_from(node_id.to_owned())?;
    let store = state.store.lock().await;
    let membership = store
        .state()
        .repository_membership
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("history repository membership is not configured"))?;
    let member = membership
        .repository(&node_id)
        .ok_or_else(|| anyhow::anyhow!("relay target is not in repository membership"))?;
    Ok(*member.identity().x25519_relay_public_key().as_bytes())
}

async fn replicate_peer(
    state: &AppState,
    peer: &MeshPeerTarget,
    ready_repository_ids: &[String],
    now: u64,
) -> anyhow::Result<()> {
    let remote_summary: RepositoryReplicaSummary = repository_direct_request(
        state,
        peer,
        Method::GET,
        "/api/admin/_internal/history-repository/summary",
        Vec::new(),
    )
    .await?;
    let missing_segment_ids = state
        .repository_replica
        .lock()
        .await
        .missing_segment_ids(&remote_summary)?;
    if missing_segment_ids.is_empty() {
        return Ok(());
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
    propagate_tombstone_acknowledgements(state, ready_repository_ids, acknowledgements).await
}

async fn propagate_tombstone_acknowledgements(
    state: &AppState,
    ready_repository_ids: &[String],
    acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
) -> anyhow::Result<()> {
    if acknowledgements.is_empty() {
        return Ok(());
    }
    let (_, peers) = ready_repository_peers(state).await?;
    let body = serde_json::to_vec(&RepositoryTombstoneAcknowledgementRequest { acknowledgements })?;
    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
        .filter(|peer| ready_repository_ids.iter().any(|id| id == &peer.node_id))
        .take(MAX_REPOSITORY_PEERS_PER_CYCLE)
    {
        repository_direct_request::<serde_json::Value>(
            state,
            peer,
            Method::POST,
            "/api/admin/_internal/history-repository/tombstone-ack",
            body.clone(),
        )
        .await?;
    }
    Ok(())
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

pub(super) async fn repository_direct_request<T>(
    state: &AppState,
    peer: &MeshPeerTarget,
    method: Method,
    path_and_query: &str,
    body: Vec<u8>,
) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let mut last_error = None::<anyhow::Error>;
    for path in [
        PeerDirectPath::RealityMesh,
        PeerDirectPath::CloudflareTunnel,
    ] {
        let content_type = (!body.is_empty()).then(|| "application/json".to_owned());
        let request = MeshRequest {
            method: method.clone(),
            path_and_query: path_and_query.to_owned(),
            content_type,
            body: body.clone(),
            total_budget: REPOSITORY_REQUEST_BUDGET,
            allow_ambiguous_fallback: true,
            request_id: crate::id::new_ulid_string(),
            route: InternalRoute::MeshV2,
            cluster_id: state.cluster.cluster_id.clone(),
            sender_id: state.cluster.node_id.clone(),
            updates_active_path: true,
        };
        match state
            .mesh_client
            .send_peer_direct_request(
                peer,
                path,
                request,
                state
                    .cluster_ca_key_pem
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("cluster CA key is not available"))?,
                &state.cluster_ca_pem,
            )
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(response) => match response.json::<T>().await {
                    Ok(decoded) => return Ok(decoded),
                    Err(error) => last_error = Some(error.into()),
                },
                Err(error) => last_error = Some(error.into()),
            },
            Err(error) => last_error = Some(error.into()),
        }
    }
    Err(anyhow::anyhow!(
        "both peer-direct repository paths failed: {}",
        last_error.expect("both direct paths were attempted")
    ))
}
