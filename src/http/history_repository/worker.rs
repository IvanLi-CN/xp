use std::time::Duration;

use axum::http::Method;
use serde::de::DeserializeOwned;
use sha2::{Digest as _, Sha256};
use tokio::time::MissedTickBehavior;

use crate::{
    control_plane_mesh::{MeshPeerTarget, MeshRequest, PeerDirectPath, peer_target_from_node},
    history_sync::{RelayFrame, RelayKeypair},
    internal_auth::InternalRoute,
    state::history_repository::replica::{
        ReplicaWork, RepositoryReplicaSummary, RepositoryTombstoneAcknowledgement,
    },
};

use super::super::AppState;
use super::{
    INTERNAL_HISTORY_REPOSITORY_RELAY, RepositoryRelayRequest, RepositoryRepairRequest,
    RepositoryTombstoneAcknowledgementRequest,
};

const REPOSITORY_REPLICATION_INTERVAL: Duration = Duration::from_secs(5 * 60);
const REPOSITORY_REQUEST_BUDGET: Duration = Duration::from_secs(15);
const MAX_REPOSITORY_PEERS_PER_CYCLE: usize = 4;
const CLUSTER_RELAY_KEY_CONTEXT: &[u8] = b"xp-history-repository-relay-key-v1\0";

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
    let known_source_node_ids = known_history_source_node_ids(state).await;
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
        runtime.record_stale_collection_cycles(
            now,
            &ready_repository_ids,
            &state.cluster.node_id,
            &known_source_node_ids,
        )?;
        runtime.replication_work(now)
    };
    if !work.is_anti_entropy() {
        return Ok(());
    }

    let peers_to_replicate = peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
        .take(MAX_REPOSITORY_PEERS_PER_CYCLE)
        .collect::<Vec<_>>();
    let mut synchronized = false;
    let ready_peer_count = peers.len().saturating_sub(1);
    let mut directly_verified_peers = 0usize;
    for peer in peers_to_replicate {
        match replicate_peer(state, peer, &ready_repository_ids, now, work).await {
            Ok(()) => {
                synchronized = true;
                directly_verified_peers = directly_verified_peers.saturating_add(1);
            }
            Err(error) => {
                tracing::debug!(
                    peer = %peer.node_id,
                    error = %error,
                    "history repository peer replication failed"
                );
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
        let deep_verification_succeeded =
            all_ready_peers_were_directly_verified(work, ready_peer_count, directly_verified_peers);
        let completed_work = completed_replication_work(work, deep_verification_succeeded);
        state
            .repository_replica
            .lock()
            .await
            .record_replication_completed(now, completed_work)?;
    }
    Ok(())
}

fn all_ready_peers_were_directly_verified(
    work: ReplicaWork,
    ready_peer_count: usize,
    directly_verified_peers: usize,
) -> bool {
    work.is_deep_verification() && ready_peer_count == directly_verified_peers
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
    let batch = {
        let mut runtime = state.repository_replica.lock().await;
        if !runtime.begin_dynamic_relay_attempt(now)? {
            anyhow::bail!("hourly jittered dynamic relay attempt is not due");
        }
        runtime.relay_batch(&target.node_id)?
    };
    if batch.segments.is_empty() && batch.gaps.is_empty() {
        return Ok(());
    }
    let delivered_segments = batch.segments.len();
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
        .record_relay_batch_delivered(&target.node_id, delivered_segments)?;
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
) -> anyhow::Result<()> {
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
    state
        .repository_replica
        .lock()
        .await
        .merge_replica_gaps(&repair.gaps)?;
    propagate_tombstone_acknowledgements(state, ready_repository_ids, acknowledgements).await
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

    #[test]
    fn daily_deep_verification_requires_every_ready_peer_directly() {
        assert!(!super::all_ready_peers_were_directly_verified(
            ReplicaWork::DeepVerification,
            2,
            1,
        ));
        assert!(super::all_ready_peers_were_directly_verified(
            ReplicaWork::DeepVerification,
            2,
            2,
        ));
        assert!(!super::all_ready_peers_were_directly_verified(
            ReplicaWork::AntiEntropy,
            1,
            1,
        ));
    }
}
