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
use crate::domain::Node;

pub(super) const MEMBERSHIP_LIFECYCLE_CAPABILITY: &str = "cluster.membership-lifecycle-v1";
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

async fn require_capability_on_voters(
    state: &AppState,
    capability: &str,
    excluded_voter_id: Option<u64>,
) -> Result<(), ApiError> {
    let metrics = raft_metrics(state);
    let membership = metrics.membership_config.membership();
    let mut voter_ids = membership.voter_ids().collect::<BTreeSet<_>>();
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
        let response = match response {
            MeshCapabilityProbeResponse::Verified(response) => response,
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
                    })?
            }
        };
        let supports_capability = if response.status().is_success() {
            match remaining_probe_budget(started) {
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

#[cfg(test)]
#[path = "join_capability_tests.rs"]
mod tests;
