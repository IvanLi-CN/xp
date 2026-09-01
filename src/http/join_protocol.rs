use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use super::{ApiError, AppState, ClusterJoinRequest, raft_metrics};
use crate::{
    domain::{Endpoint, Node},
    managed_default_endpoints::managed_default_vless_endpoint,
    reverse_mesh::{
        ReverseMeshAssignment, ReverseMeshBootstrapEndpoint, ReverseMeshBootstrapMarker,
        ReverseMeshCandidate,
    },
};

pub(super) async fn bootstrap_sender_ids(state: &AppState) -> Vec<String> {
    let voter_ids = raft_metrics(state)
        .membership_config
        .membership()
        .voter_ids()
        .collect::<BTreeSet<_>>();
    let store = state.store.lock().await;
    let mut sender_ids = store
        .list_nodes()
        .into_iter()
        .filter_map(|node| {
            crate::raft::types::raft_node_id_from_ulid(&node.node_id)
                .ok()
                .filter(|raft_id| voter_ids.contains(raft_id))
                .map(|_| node.node_id)
        })
        .collect::<BTreeSet<_>>();
    sender_ids.insert(state.cluster.node_id.clone());
    sender_ids.into_iter().collect()
}

pub(super) struct BootstrapReservation {
    pub existing: Option<crate::join_session::JoinSession>,
    pub activation_deadline: String,
    pub signed_cert_pem: String,
}

pub(super) async fn prepare_reverse_bootstrap(
    state: &AppState,
    target_node_id: &str,
) -> Result<Option<ReverseMeshBootstrapMarker>, ApiError> {
    if raft_metrics(state)
        .membership_config
        .membership()
        .voter_ids()
        .count()
        < 2
    {
        return Ok(None);
    }
    let current_epoch = state.store.lock().await.state().reverse_mesh_epoch;
    let pending_epoch = if current_epoch == 0 {
        // Reverse is additive to join. A cluster that has not completed the rolling capability
        // barrier keeps the existing public/Raft bootstrap path and receives no marker.
        if super::join_capability::require_reverse_assignment_on_voters(state)
            .await
            .is_err()
        {
            return Ok(None);
        }
        Some(reverse_membership_revision(&raft_metrics(state)))
    } else {
        None
    };
    let epoch = pending_epoch.unwrap_or(current_epoch);
    let voter_nodes = current_voter_nodes(state).await;
    let readiness = reverse_bootstrap_readiness(state, &voter_nodes).await;
    let marker = reverse_bootstrap_marker_at(state, target_node_id, epoch, &readiness).await;
    if current_epoch == 0 {
        if marker.is_none() {
            return Ok(None);
        }
        state
            .raft
            .client_write(crate::state::DesiredStateCommand::SetReverseMeshEpoch { epoch })
            .await
            .map_err(|error| ApiError::conflict(error.to_string()))?;
    }
    let Some(marker) = marker else {
        return Ok(None);
    };
    let expected_generation = state
        .store
        .lock()
        .await
        .state()
        .reverse_mesh_assignments
        .get(target_node_id)
        .map(|assignment| assignment.generation);
    let already_present = expected_generation == Some(marker.generation);
    if !already_present {
        let assignment = ReverseMeshAssignment {
            target_node_id: marker.target_node_id.clone(),
            generation: marker.generation,
            membership_revision: reverse_membership_revision(&raft_metrics(state)),
            primary_node_id: marker.primary_node_id.clone(),
            standby_node_id: marker.standby_node_id.clone(),
            credential_epoch: marker.epoch,
        };
        state
            .raft
            .client_write(
                crate::state::DesiredStateCommand::UpsertReverseMeshAssignment {
                    assignment,
                    expected_generation,
                },
            )
            .await
            .map_err(|error| ApiError::conflict(error.to_string()))?;
    }
    Ok(Some(marker))
}

pub(super) async fn reverse_bootstrap_marker(
    state: &AppState,
    target_node_id: &str,
) -> Option<ReverseMeshBootstrapMarker> {
    let epoch = state.store.lock().await.state().reverse_mesh_epoch;
    let voter_nodes = current_voter_nodes(state).await;
    let readiness = reverse_bootstrap_readiness(state, &voter_nodes).await;
    reverse_bootstrap_marker_at(state, target_node_id, epoch, &readiness).await
}

async fn reverse_bootstrap_marker_at(
    state: &AppState,
    target_node_id: &str,
    epoch: u64,
    readiness: &BTreeMap<String, bool>,
) -> Option<ReverseMeshBootstrapMarker> {
    let metrics = raft_metrics(state);
    let voter_ids = metrics
        .membership_config
        .membership()
        .voter_ids()
        .collect::<BTreeSet<_>>();
    let (nodes, endpoints, existing) = {
        let store = state.store.lock().await;
        (
            store.list_nodes(),
            store.list_endpoints(),
            store
                .state()
                .reverse_mesh_assignments
                .get(target_node_id)
                .cloned(),
        )
    };
    if epoch == 0 {
        return None;
    }
    let voter_nodes = nodes
        .iter()
        .filter(|node| {
            crate::raft::types::raft_node_id_from_ulid(&node.node_id)
                .ok()
                .is_some_and(|raft_id| voter_ids.contains(&raft_id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let candidates = voter_nodes
        .iter()
        .map(|node| ReverseMeshCandidate {
            node_id: node.node_id.clone(),
            assignment_capable: readiness.get(&node.node_id).copied().unwrap_or(false),
            relay_capable: readiness.get(&node.node_id).copied().unwrap_or(false),
            signed_xray_ready: readiness.get(&node.node_id).copied().unwrap_or(false),
            managed_vless_endpoint: reverse_endpoint_for(&node.node_id, &voter_nodes, &endpoints)
                .is_some(),
        })
        .collect::<Vec<_>>();
    let generation_counters = state
        .store
        .lock()
        .await
        .state()
        .reverse_mesh_generation_counters
        .clone();
    let assignment = existing
        .filter(|assignment| {
            assignment.credential_epoch == epoch
                && assignment.is_valid()
                && readiness
                    .get(&assignment.primary_node_id)
                    .copied()
                    .unwrap_or(false)
                && assignment
                    .standby_node_id
                    .as_deref()
                    .is_none_or(|standby| readiness.get(standby).copied().unwrap_or(false))
        })
        .or_else(|| {
            crate::reverse_mesh::assign_reverse_mesh_with_generation_floors(
                [target_node_id.to_string()],
                &candidates,
                &BTreeMap::new(),
                &generation_counters,
                reverse_membership_revision(&metrics),
                epoch,
            )
            .remove(target_node_id)
        })?;
    let primary_endpoint =
        reverse_endpoint_for(&assignment.primary_node_id, &voter_nodes, &endpoints)?;
    let standby_endpoint = assignment
        .standby_node_id
        .as_deref()
        .and_then(|node_id| reverse_endpoint_for(node_id, &voter_nodes, &endpoints));
    Some(ReverseMeshBootstrapMarker {
        epoch,
        generation: assignment.generation.max(1),
        target_node_id: target_node_id.to_string(),
        primary_node_id: assignment.primary_node_id,
        standby_node_id: assignment.standby_node_id,
        primary_endpoint,
        standby_endpoint,
    })
}

async fn current_voter_nodes(state: &AppState) -> Vec<Node> {
    let voter_ids = raft_metrics(state)
        .membership_config
        .membership()
        .voter_ids()
        .collect::<BTreeSet<_>>();
    state
        .store
        .lock()
        .await
        .list_nodes()
        .into_iter()
        .filter(|node| {
            crate::raft::types::raft_node_id_from_ulid(&node.node_id)
                .ok()
                .is_some_and(|raft_id| voter_ids.contains(&raft_id))
        })
        .collect()
}

async fn reverse_bootstrap_readiness(state: &AppState, nodes: &[Node]) -> BTreeMap<String, bool> {
    let mut readiness = BTreeMap::new();
    for node in nodes {
        let ready = super::mesh::reverse_candidate_readiness(state, node)
            .await
            .unwrap_or(false);
        readiness.insert(node.node_id.clone(), ready);
    }
    readiness
}

fn reverse_endpoint_for(
    node_id: &str,
    nodes: &[Node],
    endpoints: &[Endpoint],
) -> Option<ReverseMeshBootstrapEndpoint> {
    let endpoint = endpoints
        .iter()
        .filter(|endpoint| endpoint.node_id == node_id)
        .filter_map(|endpoint| {
            managed_default_vless_endpoint(endpoint).map(|meta| (endpoint, meta))
        })
        .min_by(|(left, _), (right, _)| left.tag.cmp(&right.tag))?;
    let (endpoint, meta) = endpoint;
    let node = nodes.iter().find(|node| node.node_id == node_id)?;
    let server_name = meta.reality.server_names.first()?.clone();
    let transport = serde_json::to_string(&meta.transport)
        .ok()?
        .trim_matches('"')
        .to_string();
    Some(ReverseMeshBootstrapEndpoint {
        access_host: node.access_host.clone(),
        port: endpoint.port,
        server_name,
        public_key: meta.reality_keys.public_key,
        short_id: meta.active_short_id,
        transport,
    })
}

fn reverse_membership_revision(
    metrics: &openraft::RaftMetrics<crate::raft::types::NodeId, crate::raft::types::NodeMeta>,
) -> u64 {
    let revision = crate::raft_membership_guard::membership_revision(metrics).unwrap_or_default();
    let digest = Sha256::digest(revision.as_bytes());
    u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("membership digest has eight bytes"),
    )
    .max(1)
}

pub(super) async fn resolve_reservation(
    state: &AppState,
    req: &ClusterJoinRequest,
    token: &crate::cluster_identity::JoinToken,
    request_fingerprint: &str,
    ca_key_pem: &str,
) -> Result<BootstrapReservation, ApiError> {
    super::resource_history_capacity::preflight_resource_history_capacity_for_join(
        state,
        &token.token_id,
    )
    .await?;
    let existing = state
        .store
        .lock()
        .await
        .state()
        .join_sessions
        .get(&token.token_id)
        .cloned();
    if existing.is_none() {
        token
            .validate_at(Utc::now())
            .map_err(|e| ApiError::invalid_request(e.to_string()))?;
    }
    let activation_deadline = existing
        .as_ref()
        .map(|session| session.activation_deadline.clone())
        .unwrap_or_else(|| {
            (Utc::now()
                + chrono::Duration::from_std(crate::join_session::ACTIVATION_TIMEOUT)
                    .expect("join activation timeout fits chrono duration"))
            .to_rfc3339()
        });
    let signed_cert_pem = if let Some(session) = existing.as_ref() {
        if session.request_fingerprint != request_fingerprint {
            return Err(ApiError::invalid_request(
                "join token reserved by another request",
            ));
        }
        if session.status == crate::join_session::JoinSessionStatus::Expired {
            return Err(ApiError::invalid_request("join token already used"));
        }
        if session.status.is_pending()
            && DateTime::parse_from_rfc3339(&session.activation_deadline)
                .map_err(|error| ApiError::internal(error.to_string()))?
                <= Utc::now()
        {
            return Err(ApiError::invalid_request(
                "join activation deadline has expired",
            ));
        }
        session.signed_cert_pem.clone()
    } else {
        if state.store.lock().await.get_node(&token.token_id).is_some() {
            return Err(ApiError::invalid_request("join token already used"));
        }
        crate::cluster_identity::sign_node_csr(&state.cluster.cluster_id, ca_key_pem, &req.csr_pem)
            .map_err(|e| ApiError::internal(e.to_string()))?
    };
    Ok(BootstrapReservation {
        existing,
        activation_deadline,
        signed_cert_pem,
    })
}
