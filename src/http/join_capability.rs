use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use serde::Deserialize;

use super::{ApiError, AppState, raft_metrics, send_mesh_internal_read};
use crate::domain::Node;

pub(super) const MEMBERSHIP_LIFECYCLE_CAPABILITY: &str = "cluster.membership-lifecycle-v1";
const CAPABILITY_PROBE_BUDGET: Duration = Duration::from_secs(5);
const LEGACY_CAPABILITIES_PATH: &str = "/api/capabilities";

fn remaining_probe_budget(started: Instant) -> Option<Duration> {
    let remaining = CAPABILITY_PROBE_BUDGET.saturating_sub(started.elapsed());
    (!remaining.is_zero()).then_some(remaining)
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
    if peers.len() != voter_ids.len()
        || peers
            .iter()
            .any(|peer| peer.node.api_base_url.trim().is_empty())
    {
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
        let response = send_mesh_internal_read(
            state,
            &state.mesh_client,
            &peer.node,
            "/api/admin/_internal/capabilities".to_string(),
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
        // A predecessor can have the lifecycle capability but predate the signed
        // internal route. Keep its rolling-upgrade path compatible without
        // weakening the Mesh-first probe for current voters.
        let response = if response.status() == StatusCode::NOT_FOUND {
            let Some(remaining) = remaining_probe_budget(started) else {
                return Err(ApiError::new(
                    "staged_join_capability_unavailable",
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!(
                        "cannot verify staged join support on {} through legacy public API: \
                         probe budget exhausted",
                        peer.raft_node_id
                    ),
                ));
            };
            let url = format!(
                "{}{}",
                peer.node.api_base_url.trim().trim_end_matches('/'),
                LEGACY_CAPABILITIES_PATH
            );
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
                            "cannot verify staged join support on {} through legacy public API: \
                             {error}",
                            peer.raft_node_id
                        ),
                    )
                })?
        } else {
            response
        };
        let supports_capability = if response.status().is_success() {
            match remaining_probe_budget(started) {
                Some(remaining) => tokio::time::timeout(remaining, response.json::<Response>())
                    .await
                    .ok()
                    .and_then(Result::ok)
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
