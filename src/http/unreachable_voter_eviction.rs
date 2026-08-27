use std::collections::BTreeSet;

use axum::{Json, extract::Extension};
use serde::{Deserialize, Serialize};

use crate::raft::types::NodeId as RaftNodeId;

use super::{
    ApiError, ApiJson, AppState, InternalSignatureAuth, join_capability, node_delete,
    spawn_admin_node_delete_resume,
};
use node_delete::AdminNodeDeletePreviewEndpoint;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InternalEvictUnreachableVoterRequest {
    node_id: String,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    expected_membership: Option<String>,
    #[serde(default)]
    delete_endpoints: bool,
    #[serde(default)]
    expected_endpoint_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct InternalEvictUnreachableVoterResponse {
    dry_run: bool,
    node_id: String,
    raft_node_id: RaftNodeId,
    expected_membership: String,
    endpoints: Vec<AdminNodeDeletePreviewEndpoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<crate::state::MembershipOperation>,
}

pub(super) async fn admin_internal_evict_unreachable_voter(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(req): ApiJson<InternalEvictUnreachableVoterRequest>,
) -> Result<Json<InternalEvictUnreachableVoterResponse>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    let preview = crate::raft_membership_guard::preview_unreachable_voter_eviction(
        state.raft.clone(),
        state.store.clone(),
        &req.node_id,
    )
    .await
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    let endpoints = {
        let store = state.store.lock().await;
        let mut endpoints = store
            .list_endpoints()
            .into_iter()
            .filter(|endpoint| endpoint.node_id == req.node_id)
            .map(|endpoint| AdminNodeDeletePreviewEndpoint {
                endpoint_id: endpoint.endpoint_id,
                tag: endpoint.tag,
                kind: endpoint.kind,
                port: endpoint.port,
            })
            .collect::<Vec<_>>();
        endpoints.sort_by(|left, right| left.endpoint_id.cmp(&right.endpoint_id));
        endpoints
    };
    join_capability::require_membership_lifecycle_on_retained_voters(&state, preview.raft_node_id)
        .await?;

    if !req.apply {
        return Ok(Json(InternalEvictUnreachableVoterResponse {
            dry_run: true,
            node_id: preview.node_id,
            raft_node_id: preview.raft_node_id,
            expected_membership: preview.expected_membership,
            endpoints,
            operation: None,
        }));
    }

    let expected_membership = req
        .expected_membership
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::invalid_request("--apply requires expected_membership"))?;
    if !req.delete_endpoints {
        return Err(ApiError::invalid_request(
            "--apply requires delete_endpoints confirmation",
        ));
    }
    let expected_endpoint_ids = req.expected_endpoint_ids;
    let expected_endpoint_ids_set = expected_endpoint_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if expected_endpoint_ids_set.len() != expected_endpoint_ids.len() {
        return Err(ApiError::invalid_request(
            "expected_endpoint_ids must not contain duplicates",
        ));
    }
    if expected_endpoint_ids_set != preview.endpoint_ids.iter().cloned().collect() {
        return Err(ApiError::conflict(
            "node endpoint set changed since unreachable voter eviction preview",
        ));
    }
    let operation = crate::raft_membership_guard::begin_unreachable_voter_eviction(
        state.raft.clone(),
        state.store.clone(),
        &req.node_id,
        &expected_membership,
        req.delete_endpoints,
        expected_endpoint_ids,
    )
    .await
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    spawn_admin_node_delete_resume(state.clone());
    Ok(Json(InternalEvictUnreachableVoterResponse {
        dry_run: false,
        node_id: preview.node_id,
        raft_node_id: preview.raft_node_id,
        expected_membership: operation.expected_membership.clone(),
        endpoints,
        operation: Some(operation),
    }))
}
