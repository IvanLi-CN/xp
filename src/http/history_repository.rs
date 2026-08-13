use axum::{
    Json,
    extract::{Extension, Query},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use serde::Deserialize;

use crate::{
    history_sync::MAX_RESPONSE_WIRE_BYTES,
    http::{ApiError, ApiJson, AppState, InternalSignatureAuth},
    state::history_repository::{
        identity::RepositoryNodeIdentity,
        query::HistoryQuery,
        replica::{
            LocalQueryMetadata, RepositoryHistoryQueryResponse, RepositoryRuntimeError,
            RepositorySyncReceipt,
        },
    },
};

const MAX_HISTORY_SYNC_BASE64_BYTES: usize = MAX_RESPONSE_WIRE_BYTES.div_ceil(3) * 4;

#[derive(Debug, Deserialize)]
pub(super) struct RepositoryHistoryQuery {
    start_unix_seconds: u64,
    end_unix_seconds: u64,
    page_size: usize,
}

#[derive(Debug, Deserialize)]
pub(super) struct RepositorySyncRequest {
    identity: RepositoryNodeIdentity,
    wire_base64: String,
}

pub(super) async fn admin_internal_receive_history_repository_segment(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositorySyncRequest>,
) -> Result<Json<RepositorySyncReceipt>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if request.identity.node_id().as_str() != verified.context.sender_id {
        return Err(ApiError::unauthorized(
            "repository segment identity does not match internal sender",
        ));
    }
    if request.wire_base64.len() > MAX_HISTORY_SYNC_BASE64_BYTES {
        return Err(ApiError::invalid_request(
            "repository segment exceeds wire limit",
        ));
    }
    let wire = URL_SAFE_NO_PAD
        .decode(request.wire_base64)
        .map_err(|_| ApiError::invalid_request("repository segment is not base64url"))?;
    let receipt = state
        .repository_replica
        .lock()
        .await
        .receive_wire(
            &state.cluster.cluster_id,
            &request.identity,
            &wire,
            u64::try_from(Utc::now().timestamp()).unwrap_or_default(),
        )
        .map_err(repository_error)?;
    Ok(Json(receipt))
}

pub(super) async fn admin_query_history_repository(
    Extension(state): Extension<AppState>,
    Query(request): Query<RepositoryHistoryQuery>,
) -> Result<Json<RepositoryHistoryQueryResponse>, ApiError> {
    let query = HistoryQuery::new(
        request.start_unix_seconds,
        request.end_unix_seconds,
        request.page_size,
    )
    .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    let now = u64::try_from(Utc::now().timestamp()).unwrap_or_default();
    let response = state
        .repository_replica
        .lock()
        .await
        .query(
            &state.cluster.node_id,
            query,
            LocalQueryMetadata::current_window(now),
        )
        .map_err(repository_error)?;
    Ok(Json(response))
}

fn repository_error(error: RepositoryRuntimeError) -> ApiError {
    match error {
        RepositoryRuntimeError::Query(error) => ApiError::invalid_request(error.to_string()),
        RepositoryRuntimeError::WriteStopped(_) => {
            ApiError::conflict("repository history writes are temporarily stopped")
        }
        RepositoryRuntimeError::Protocol(error) => ApiError::invalid_request(error.to_string()),
        RepositoryRuntimeError::Replica(error) => ApiError::invalid_request(error.to_string()),
        RepositoryRuntimeError::ClusterBindingMismatch => {
            ApiError::unauthorized("repository cluster binding does not match")
        }
        RepositoryRuntimeError::StateLimitExceeded => {
            ApiError::conflict("repository history capacity is exhausted")
        }
        RepositoryRuntimeError::Storage(error) => ApiError::internal(error),
    }
}
