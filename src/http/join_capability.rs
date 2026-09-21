use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use futures_util::StreamExt;
use serde::Deserialize;

use super::{
    ApiError, AppState, MeshCapabilityProbeResponse, raft_metrics,
    send_mesh_internal_capability_read,
};
use crate::{
    control_plane_mesh::{MeshPeerTarget, MeshRequest, PeerDirectPath},
    domain::Node,
};

pub(super) const MEMBERSHIP_LIFECYCLE_CAPABILITY: &str = "cluster.membership-lifecycle-v1";
pub(super) const STALE_LEARNER_RETIREMENT_CAPABILITY: &str = "cluster.stale-learner-retirement-v1";
pub(super) const REVERSE_ASSIGNMENT_CAPABILITY: &str = "cluster.mesh-reverse-assignment-v1";
pub(super) const MESH_GATE_CAPABILITY: &str = "cluster.mesh-gate-v1";
const CAPABILITY_PROBE_BUDGET: Duration = Duration::from_secs(5);
const MAX_CAPABILITY_RESPONSE_BYTES: usize = 64 * 1024;
const LEGACY_CAPABILITIES_PATH: &str = "/api/capabilities";

fn remaining_probe_budget(started: Instant) -> Option<Duration> {
    let remaining = CAPABILITY_PROBE_BUDGET.saturating_sub(started.elapsed());
    (!remaining.is_zero()).then_some(remaining)
}

async fn read_capability_response(
    response: reqwest::Response,
    budget: Duration,
) -> Option<Response> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CAPABILITY_RESPONSE_BYTES as u64)
    {
        return None;
    }
    let bytes = tokio::time::timeout(budget, async move {
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.ok()?;
            let next_len = bytes.len().checked_add(chunk.len())?;
            if next_len > MAX_CAPABILITY_RESPONSE_BYTES {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        Some(bytes)
    })
    .await
    .ok()
    .flatten()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Deserialize)]
struct Response {
    capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
struct VoterCapabilityPeer {
    raft_node_id: u64,
    node: Node,
}

pub(super) async fn require_membership_lifecycle_on_voters(
    state: &AppState,
) -> Result<(), ApiError> {
    require_capability_on_voters(state, MEMBERSHIP_LIFECYCLE_CAPABILITY, None).await
}

pub(super) async fn require_membership_lifecycle_on_retained_voters(
    state: &AppState,
    excluded_voter_id: u64,
) -> Result<(), ApiError> {
    require_capability_on_voters(
        state,
        MEMBERSHIP_LIFECYCLE_CAPABILITY,
        Some(excluded_voter_id),
    )
    .await
}

pub(super) async fn require_stale_learner_retirement_on_voters(
    state: &AppState,
) -> Result<(), ApiError> {
    require_capability_on_voters(state, STALE_LEARNER_RETIREMENT_CAPABILITY, None).await
}

pub(super) async fn require_reverse_assignment_on_voters(state: &AppState) -> Result<(), ApiError> {
    require_capability_on_voters(state, REVERSE_ASSIGNMENT_CAPABILITY, None).await
}

pub(super) async fn require_mesh_gate_on_voters(state: &AppState) -> Result<(), ApiError> {
    require_capability_on_voters_with_probe(state, MESH_GATE_CAPABILITY, None, true).await
}

async fn require_capability_on_voters(
    state: &AppState,
    capability: &str,
    excluded_voter_id: Option<u64>,
) -> Result<(), ApiError> {
    require_capability_on_voters_with_probe(state, capability, excluded_voter_id, false).await
}

async fn require_capability_on_voters_with_probe(
    state: &AppState,
    capability: &str,
    excluded_voter_id: Option<u64>,
    public_only: bool,
) -> Result<(), ApiError> {
    let metrics = raft_metrics(state);
    let membership = metrics.membership_config.membership();
    let mut voter_ids = membership.voter_ids().collect::<BTreeSet<_>>();
    if public_only {
        voter_ids.extend(membership.nodes().map(|(node_id, _)| *node_id));
    }
    let local_node_id = crate::raft::types::raft_node_id_from_ulid(&state.cluster.node_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    voter_ids.remove(&local_node_id);
    if let Some(excluded_voter_id) = excluded_voter_id {
        voter_ids.remove(&excluded_voter_id);
    }

    let nodes_by_raft_node_id = {
        let store = state.store.lock().await;
        store
            .list_nodes()
            .into_iter()
            .filter_map(|node| {
                crate::raft::types::raft_node_id_from_ulid(&node.node_id)
                    .ok()
                    .map(|raft_node_id| (raft_node_id, node))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let peers = voter_ids
        .iter()
        .filter_map(|raft_node_id| {
            membership.get_node(raft_node_id).and_then(|_| {
                nodes_by_raft_node_id
                    .get(raft_node_id)
                    .cloned()
                    .map(|node| VoterCapabilityPeer {
                        raft_node_id: *raft_node_id,
                        node,
                    })
            })
        })
        .collect::<Vec<_>>();
    if peers.len() != voter_ids.len() {
        return Err(ApiError::new(
            "coordinated_upgrade_required",
            StatusCode::CONFLICT,
            "every retained voter must expose valid Raft member metadata and DesiredState \
             mapping before membership changes",
        ));
    }
    if voter_ids.is_empty() {
        return Ok(());
    }
    for peer in peers {
        let started = Instant::now();
        if public_only {
            if !public_capability_supports(state, &peer.node, capability, started).await {
                return Err(ApiError::new(
                    "coordinated_upgrade_required",
                    StatusCode::CONFLICT,
                    format!(
                        "member {} must expose {capability} before this cluster setting can change",
                        peer.raft_node_id
                    ),
                ));
            }
            continue;
        }
        let response = send_mesh_internal_capability_read(
            state,
            &state.mesh_client,
            &peer.node,
            CAPABILITY_PROBE_BUDGET,
        )
        .await
        .map_err(|error| {
            ApiError::new(
                "staged_join_capability_unavailable",
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "cannot verify staged join support on {} through Mesh: {}",
                    peer.raft_node_id, error.message
                ),
            )
        })?;
        // Only a predecessor's unsigned 404 proves that it predates the signed
        // route. A signed 404, a protocol error, or a Mesh transport failure is
        // terminal and must not move this probe onto a public path.
        let (response, mesh_remaining) = match response {
            MeshCapabilityProbeResponse::Verified { response, deadline } => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(ApiError::new(
                        "staged_join_capability_unavailable",
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!(
                            "cannot verify staged join support on {} through Mesh: probe budget {}",
                            peer.raft_node_id, "exhausted"
                        ),
                    ));
                }
                (response, Some(remaining))
            }
            MeshCapabilityProbeResponse::PredecessorNotFound => {
                let Some(remaining) = remaining_probe_budget(started) else {
                    return Err(ApiError::new(
                        "staged_join_capability_unavailable",
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!(
                            "cannot verify staged join support on {} through \
                             legacy public API: probe budget exhausted",
                            peer.raft_node_id
                        ),
                    ));
                };
                let api_base_url = peer.node.api_base_url.trim().trim_end_matches('/');
                if api_base_url.is_empty() {
                    return Err(ApiError::new(
                        "staged_join_capability_unavailable",
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!(
                            "cannot verify staged join support on {} through legacy public API: \
                             public API URL is not configured",
                            peer.raft_node_id
                        ),
                    ));
                }
                let url = format!("{api_base_url}{LEGACY_CAPABILITIES_PATH}");
                (
                    state
                        .mesh_client
                        .direct()
                        .get(url)
                        .timeout(remaining)
                        .send()
                        .await
                        .map_err(|error| {
                            ApiError::new(
                                "staged_join_capability_unavailable",
                                StatusCode::SERVICE_UNAVAILABLE,
                                format!(
                                    "cannot verify staged join support on {} through \
                                 legacy public API: {error}",
                                    peer.raft_node_id
                                ),
                            )
                        })?,
                    None,
                )
            }
        };
        let supports_capability = if response.status().is_success() {
            match mesh_remaining.or_else(|| remaining_probe_budget(started)) {
                Some(remaining) => read_capability_response(response, remaining)
                    .await
                    .is_some_and(|body| body.capabilities.iter().any(|item| item == capability)),
                None => false,
            }
        } else {
            false
        };
        if !supports_capability {
            return Err(ApiError::new(
                "coordinated_upgrade_required",
                StatusCode::CONFLICT,
                "all voters must be upgraded before membership changes",
            ));
        }
    }
    Ok(())
}

async fn public_capability_supports(
    state: &AppState,
    node: &Node,
    capability: &str,
    started: Instant,
) -> bool {
    let remaining = remaining_probe_budget(started).unwrap_or_default();
    if remaining.is_zero() {
        return false;
    }
    let api_base_url = node.api_base_url.trim().trim_end_matches('/');
    if api_base_url.is_empty() {
        return false;
    }
    let peer = MeshPeerTarget {
        node_id: node.node_id.clone(),
        node_name: node.node_name.clone(),
        mesh_base_url: None,
        endpoint_transport: None,
        endpoint_fingerprint: None,
        mesh_reason: crate::mesh_telemetry::MeshPeerReason::MissingEndpoint,
        public_base_url: api_base_url.to_string(),
    };
    let request = MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/capabilities".to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: remaining,
        allow_ambiguous_fallback: false,
        request_id: crate::id::new_ulid_string(),
        route: crate::internal_auth::InternalRoute::MeshV2,
        cluster_id: state.cluster.cluster_id.clone(),
        sender_id: state.cluster.node_id.clone(),
        updates_active_path: false,
    };
    let Some(ca_key_pem) = state.cluster_ca_key_pem.as_deref() else {
        return false;
    };
    let response = state
        .mesh_client
        .send_peer_direct_request(
            &peer,
            PeerDirectPath::ApiBaseUrl,
            request,
            ca_key_pem,
            &state.cluster_ca_pem,
        )
        .await
        .ok();
    let Some(response) = response else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    let Some(remaining) = remaining_probe_budget(started) else {
        return false;
    };
    read_capability_response(response, remaining)
        .await
        .is_some_and(|body| body.capabilities.iter().any(|item| item == capability))
}

#[cfg(test)]
#[path = "join_capability_tests.rs"]
mod tests;
