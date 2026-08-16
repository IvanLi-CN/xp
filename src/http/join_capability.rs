use std::{collections::BTreeSet, time::Duration};

use axum::http::StatusCode;
use serde::Deserialize;

use super::{ApiError, AppState, raft_metrics};

pub(super) const MEMBERSHIP_LIFECYCLE_CAPABILITY: &str = "cluster.membership-lifecycle-v1";

#[derive(Deserialize)]
struct Response {
    capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
struct VoterCapabilityPeer {
    raft_node_id: u64,
    api_base_url: String,
}

pub(super) async fn require_membership_lifecycle_on_voters(
    state: &AppState,
) -> Result<(), ApiError> {
    require_capability_on_voters(state, MEMBERSHIP_LIFECYCLE_CAPABILITY).await
}

async fn require_capability_on_voters(state: &AppState, capability: &str) -> Result<(), ApiError> {
    let metrics = raft_metrics(state);
    let membership = metrics.membership_config.membership();
    let mut voter_ids = membership.voter_ids().collect::<BTreeSet<_>>();
    let local_node_id = crate::raft::types::raft_node_id_from_ulid(&state.cluster.node_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    voter_ids.remove(&local_node_id);
    let peers = voter_ids
        .iter()
        .filter_map(|raft_node_id| {
            membership
                .get_node(raft_node_id)
                .map(|node| VoterCapabilityPeer {
                    raft_node_id: *raft_node_id,
                    api_base_url: node.api_base_url.clone(),
                })
        })
        .collect::<Vec<_>>();
    if peers.len() != voter_ids.len()
        || peers.iter().any(|peer| peer.api_base_url.trim().is_empty())
    {
        return Err(ApiError::new(
            "coordinated_upgrade_required",
            StatusCode::CONFLICT,
            "every voter must expose valid Raft member metadata before membership changes",
        ));
    }
    if voter_ids.is_empty() {
        return Ok(());
    }
    let client =
        crate::ops::build_xp_ops_http_client(&peers[0].api_base_url, state.cluster_ca_pem.as_str())
            .map_err(|error| ApiError::internal(error.message))?;
    for peer in peers {
        let url = format!(
            "{}/api/capabilities",
            peer.api_base_url.trim_end_matches('/')
        );
        let response = client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|error| {
                ApiError::new(
                    "staged_join_capability_unavailable",
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!(
                        "cannot verify staged join support on {}: {error}",
                        peer.raft_node_id
                    ),
                )
            })?;
        if !response.status().is_success()
            || !response
                .json::<Response>()
                .await
                .is_ok_and(|body| body.capabilities.iter().any(|item| item == capability))
        {
            return Err(ApiError::new(
                "coordinated_upgrade_required",
                StatusCode::CONFLICT,
                "all voters must be upgraded before membership changes",
            ));
        }
    }
    Ok(())
}
