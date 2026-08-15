use std::{collections::BTreeSet, time::Duration};

use axum::http::StatusCode;
use serde::Deserialize;

use super::{ApiError, AppState, raft_metrics};

const CAPABILITY: &str = "cluster.join.staged-v1";

#[derive(Deserialize)]
struct Response {
    capabilities: Vec<String>,
}

fn all_remote_voters_represented(
    voter_ids: &BTreeSet<u64>,
    represented_voters: &BTreeSet<u64>,
) -> bool {
    represented_voters == voter_ids
}

pub(super) async fn require_on_voters(state: &AppState) -> Result<(), ApiError> {
    let mut voter_ids = raft_metrics(state)
        .membership_config
        .membership()
        .voter_ids()
        .collect::<BTreeSet<_>>();
    let local_node_id = crate::raft::types::raft_node_id_from_ulid(&state.cluster.node_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    voter_ids.remove(&local_node_id);
    let peers = state
        .store
        .lock()
        .await
        .list_nodes()
        .into_iter()
        .filter(|node| node.node_id != state.cluster.node_id)
        .filter(|node| {
            crate::raft::types::raft_node_id_from_ulid(&node.node_id)
                .is_ok_and(|node_id| voter_ids.contains(&node_id))
        })
        .collect::<Vec<_>>();
    let represented_voters = peers
        .iter()
        .filter_map(|node| crate::raft::types::raft_node_id_from_ulid(&node.node_id).ok())
        .collect::<BTreeSet<_>>();
    if !all_remote_voters_represented(&voter_ids, &represented_voters) {
        return Err(ApiError::conflict(
            "every voter must have valid node metadata before accepting staged joins",
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
                        peer.node_id
                    ),
                )
            })?;
        if !response.status().is_success()
            || !response
                .json::<Response>()
                .await
                .is_ok_and(|body| body.capabilities.iter().any(|item| item == CAPABILITY))
        {
            return Err(ApiError::conflict(
                "all voters must be upgraded before accepting staged joins",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_any_voter_without_probeable_node_metadata() {
        let voters = BTreeSet::from([2, 3]);
        assert!(all_remote_voters_represented(
            &voters,
            &BTreeSet::from([2, 3])
        ));
        assert!(!all_remote_voters_represented(
            &voters,
            &BTreeSet::from([2])
        ));
    }
}
