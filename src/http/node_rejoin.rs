//! Operator entry point for rejoining an absent DesiredState node.
//!
//! The membership change itself belongs to the durable `Restore` lifecycle, which already owns
//! learner registration, catch-up and promotion for a node that holds no Raft identity. This
//! handler adds the two things a quorum recovery needs on top of that: an identity-preserving join
//! token, so the rejoin restores the node's existing `node_id` instead of minting a new one, and an
//! explicit preview that proves the target shape before anything is written.

use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use crate::raft::types::NodeId as RaftNodeId;

use super::{ApiError, ApiJson, AppState, InternalSignatureAuth};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InternalRejoinNodeRequest {
    node_id: String,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    expected_membership: Option<String>,
    /// Token lifetime for the `xp join` the operator runs next. Ignored unless `apply`.
    #[serde(default)]
    ttl_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct InternalRejoinNodeResponse {
    dry_run: bool,
    node_id: String,
    raft_node_id: RaftNodeId,
    node_name: String,
    api_base_url: String,
    expected_membership: String,
    /// Endpoints the rejoin preserves, so the operator can confirm the data plane is untouched.
    endpoints: Vec<String>,
    /// Other absent DesiredState members this cluster still has to converge. Restoring the target
    /// is admitted while they remain absent, but they each still need a later rejoin or removal.
    absent_peers: Vec<String>,
    /// Signed join token bound to this exact node identity. Only present after `apply`.
    #[serde(skip_serializing_if = "Option::is_none")]
    join_token: Option<String>,
}

/// Whether this exact target is owned by an active `Restore` operation.
///
/// A rejoin records that operation before the node calls `xp join`; the join handler must then
/// hand back bootstrap material instead of creating a competing Join.
pub(super) async fn owns_active_restore(
    state: &AppState,
    node_id: &str,
    raft_node_id: RaftNodeId,
) -> bool {
    state
        .store
        .lock()
        .await
        .state()
        .active_membership_operation()
        .is_some_and(|operation| {
            operation.kind == crate::state::MembershipOperationKind::Restore
                && operation.raft_node_id == raft_node_id
                && operation.node_id.as_deref() == Some(node_id)
                && !operation.phase.is_terminal()
                && operation.phase != crate::state::MembershipOperationPhase::Blocked
        })
}

pub(super) async fn admin_internal_rejoin_node(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(req): ApiJson<InternalRejoinNodeRequest>,
) -> Result<Json<InternalRejoinNodeResponse>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    let preview = crate::raft_membership_guard::node_rejoin::preview_node_rejoin(
        state.raft.clone(),
        state.store.clone(),
        &req.node_id,
    )
    .await
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    let absent_peers = crate::raft_membership_guard::node_rejoin::absent_peers(
        state.raft.clone(),
        state.store.clone(),
        preview.raft_node_id,
    )
    .await
    .map_err(|error| ApiError::conflict(error.to_string()))?;

    if !req.apply {
        return Ok(Json(InternalRejoinNodeResponse {
            dry_run: true,
            node_id: preview.node_id,
            raft_node_id: preview.raft_node_id,
            node_name: preview.node_name,
            api_base_url: preview.api_base_url,
            expected_membership: preview.expected_membership,
            endpoints: preview.endpoint_ids,
            absent_peers,
            join_token: None,
        }));
    }

    let expected_membership = req
        .expected_membership
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::invalid_request("--apply requires expected_membership"))?;
    if expected_membership != preview.expected_membership {
        return Err(ApiError::conflict(
            "membership revision changed since dry-run",
        ));
    }
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_ref()
        .clone()
        .ok_or_else(|| ApiError::internal("cluster ca key is not available on this node"))?;

    // Hand the membership change to the existing Restore lifecycle, including every peer that is
    // still absent, so its allowlist check accepts this exact cluster shape and nothing wider.
    super::membership_restore::begin_absent_node_restore(
        &state,
        &req.node_id,
        &absent_peers,
        crate::raft_membership_guard::node_rejoin::rejoin_evidence(),
    )
    .await?;

    // The token id is the target's existing node id, so `xp join` on that host restores its
    // identity instead of minting a new node.
    let token = crate::cluster_identity::JoinToken::issue_signed_for_identity(
        state.cluster.cluster_id.clone(),
        state.config.api_base_url.clone(),
        state.cluster_ca_pem.as_str(),
        req.node_id.clone(),
        req.ttl_seconds.unwrap_or(900),
        chrono::Utc::now(),
        &ca_key_pem,
    );

    Ok(Json(InternalRejoinNodeResponse {
        dry_run: false,
        node_id: preview.node_id,
        raft_node_id: preview.raft_node_id,
        node_name: preview.node_name,
        api_base_url: preview.api_base_url,
        expected_membership,
        endpoints: preview.endpoint_ids,
        absent_peers,
        join_token: Some(token.encode_base64url_json()),
    }))
}
