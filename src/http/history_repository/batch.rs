use axum::{Json, extract::Extension};
use chrono::Utc;

use super::{
    ApiError, ApiJson, AppState, InternalSignatureAuth, RepositorySyncRequest, gaps,
    identity_is_pinned_for_node, repository_error, repository_identity_matches_sender,
};
use crate::state::history_repository::replica::RepositorySyncReceipt;

pub(super) const MAX_HISTORY_SYNC_BATCH_SEGMENTS: usize = 32;
pub(super) const MAX_HISTORY_SYNC_BATCH_WIRE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RepositorySyncBatchRequest {
    pub(crate) requests: Vec<RepositorySyncRequest>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct RepositorySyncBatchResponse {
    pub(crate) receipts: Vec<RepositorySyncReceipt>,
}

pub(crate) async fn admin_internal_receive_history_repository_batch(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositorySyncBatchRequest>,
) -> Result<Json<RepositorySyncBatchResponse>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if request.requests.is_empty() {
        return Err(ApiError::invalid_request(
            "repository sync batch must not be empty",
        ));
    }
    if request.requests.len() > MAX_HISTORY_SYNC_BATCH_SEGMENTS {
        return Err(ApiError::invalid_request(
            "repository sync batch exceeds segment limit",
        ));
    }
    let first = request
        .requests
        .first()
        .expect("non-empty repository sync batch");
    if !repository_identity_matches_sender(&first.identity, &verified.context.sender_id)
        || !identity_is_pinned_for_node(&state, &first.identity).await?
    {
        return Err(ApiError::unauthorized(
            "repository segment identity is not pinned for the authenticated sender",
        ));
    }
    if request
        .requests
        .iter()
        .any(|item| item.identity != first.identity)
    {
        return Err(ApiError::invalid_request(
            "repository sync batch must use one source identity",
        ));
    }
    if request
        .requests
        .iter()
        .skip(1)
        .any(|item| !item.gaps.is_empty())
    {
        return Err(ApiError::invalid_request(
            "repository sync gaps are only allowed on the first batch item",
        ));
    }
    if !gaps::source_gaps_match_identity(&first.gaps, first.identity.node_id().as_str()) {
        return Err(ApiError::invalid_request(
            "history source gaps do not match the authenticated source",
        ));
    }
    let mut segments = Vec::with_capacity(request.requests.len());
    let mut total_wire_bytes = 0_usize;
    for item in &request.requests {
        let wire = item.decode_wire()?;
        total_wire_bytes = total_wire_bytes
            .checked_add(wire.len())
            .ok_or_else(|| ApiError::invalid_request("repository sync batch wire size overflow"))?;
        if total_wire_bytes > MAX_HISTORY_SYNC_BATCH_WIRE_BYTES {
            return Err(ApiError::invalid_request(
                "repository sync batch exceeds wire limit",
            ));
        }
        segments.push(
            crate::state::history_repository::replica::RepositoryReplicaSegment {
                identity: item.identity.clone(),
                wire,
            },
        );
    }
    let (ready_repository_ids, peers, _) =
        super::worker::ready_repository_peers_with_metadata_status(&state)
            .await
            .map_err(|error| ApiError::conflict(error.to_string()))?;
    let available_ready_repository_ids =
        super::worker::available_ready_repository_ids(&ready_repository_ids, &peers);
    let accepts_source = state
        .repository_replica
        .lock()
        .await
        .accepts_source(
            first.identity.node_id().as_str(),
            &available_ready_repository_ids,
            &state.cluster.node_id,
        )
        .map_err(repository_error)?;
    if !accepts_source {
        return Err(ApiError::conflict(
            "repository is not a rendezvous collector for this source",
        ));
    }
    let receipts = {
        let mut runtime = state.repository_replica.lock().await;
        runtime
            .receive_wire_batch_from_repository_with_gaps(
                &state.cluster.cluster_id,
                &segments,
                &first.gaps,
                u64::try_from(Utc::now().timestamp()).unwrap_or_default(),
                &ready_repository_ids,
                &state.cluster.node_id,
            )
            .map_err(repository_error)?
    };
    state
        .repository_replica
        .lock()
        .await
        .record_collection_cycle(
            first.identity.node_id().as_str(),
            &ready_repository_ids,
            &state.cluster.node_id,
            true,
        )
        .map_err(repository_error)?;
    Ok(Json(RepositorySyncBatchResponse { receipts }))
}
