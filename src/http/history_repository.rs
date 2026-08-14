use axum::{
    Json,
    extract::{Extension, Query},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    history_sync::MAX_RESPONSE_WIRE_BYTES,
    http::{ApiError, ApiJson, AppState, InternalSignatureAuth},
    state::history_repository::{
        identity::RepositoryNodeIdentity,
        query::{HistoryQuery, QueryCandidate, QuerySelector},
        replica::{
            LocalQueryMetadata, RepositoryHistoryQueryResponse, RepositoryRepairBatch,
            RepositoryReplicaSummary, RepositoryRuntimeError, RepositorySyncReceipt,
            RepositoryTombstoneAcknowledgement,
        },
    },
};

const MAX_HISTORY_SYNC_BASE64_BYTES: usize = MAX_RESPONSE_WIRE_BYTES.div_ceil(3) * 4;
const MAX_REPAIR_REQUEST_IDS: usize = 64;

mod worker;
pub(crate) use worker::spawn_repository_replica_worker;

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RepositoryRepairRequest {
    segment_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RepositoryTombstoneAcknowledgementRequest {
    acknowledgements: Vec<RepositoryTombstoneAcknowledgement>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RepositoryRelayRequest {
    target_repository_id: String,
    source_repository_id: String,
    frame: crate::history_sync::RelayFrame,
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
    let ready_repository_ids = ready_repository_ids(&state).await?;
    if !ready_repository_ids
        .iter()
        .any(|repository_id| repository_id == &verified.context.sender_id)
    {
        return Err(ApiError::unauthorized(
            "repository segment sender is not ready",
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
        .receive_wire_from_repository(
            &state.cluster.cluster_id,
            &request.identity,
            &wire,
            u64::try_from(Utc::now().timestamp()).unwrap_or_default(),
            &ready_repository_ids,
            &state.cluster.node_id,
        )
        .map_err(repository_error)?;
    Ok(Json(receipt))
}

pub(super) async fn admin_internal_history_repository_summary(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<Json<RepositoryReplicaSummary>, ApiError> {
    ensure_ready_repository_sender(&state, internal).await?;
    Ok(Json(
        state.repository_replica.lock().await.replication_summary(),
    ))
}

pub(super) async fn admin_internal_history_repository_repair(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryRepairRequest>,
) -> Result<Json<RepositoryRepairBatch>, ApiError> {
    ensure_ready_repository_sender(&state, internal).await?;
    if request.segment_ids.len() > MAX_REPAIR_REQUEST_IDS {
        return Err(ApiError::invalid_request(
            "too many repository repair segment ids",
        ));
    }
    let response = state
        .repository_replica
        .lock()
        .await
        .repair_batch(&request.segment_ids)
        .map_err(repository_error)?;
    Ok(Json(response))
}

pub(super) async fn admin_internal_query_history_repository(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryHistoryQuery>,
) -> Result<Json<RepositoryHistoryQueryResponse>, ApiError> {
    ensure_ready_repository_sender(&state, internal).await?;
    let ready_repository_ids = ready_repository_ids(&state).await?;
    if !ready_repository_ids
        .iter()
        .any(|repository_id| repository_id == &state.cluster.node_id)
    {
        return Err(ApiError::conflict("repository receiver is not ready"));
    }
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

pub(super) async fn admin_internal_forward_history_repository_relay(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryRelayRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_ready_repository_sender(&state, internal).await?;
    let (_, peers) = worker::ready_repository_peers(&state)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let target = peers
        .iter()
        .find(|peer| peer.node_id == request.target_repository_id)
        .ok_or_else(|| ApiError::invalid_request("relay target is not a ready repository"))?;
    let body =
        serde_json::to_vec(&request).map_err(|error| ApiError::internal(error.to_string()))?;
    worker::repository_direct_request::<serde_json::Value>(
        &state,
        target,
        axum::http::Method::POST,
        "/_internal/history-repository/relay-deliver",
        body,
    )
    .await
    .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(Json(serde_json::json!({})))
}

pub(super) async fn admin_internal_deliver_history_repository_relay(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryRelayRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_ready_repository_sender(&state, internal).await?;
    if request.target_repository_id != state.cluster.node_id {
        return Err(ApiError::invalid_request(
            "relay delivery target does not match this repository",
        ));
    }
    let ready_repository_ids = ready_repository_ids(&state).await?;
    if !ready_repository_ids
        .iter()
        .any(|repository_id| repository_id == &request.source_repository_id)
    {
        return Err(ApiError::unauthorized("relay source is not ready"));
    }
    let batch = {
        let mut runtime = state.repository_replica.lock().await;
        let keypair = runtime.relay_keypair().map_err(repository_error)?;
        let plaintext = request
            .frame
            .open(keypair, request.target_repository_id.as_bytes())
            .map_err(|error| ApiError::unauthorized(error.to_string()))?;
        serde_json::from_slice::<RepositoryRepairBatch>(&plaintext)
            .map_err(|_| ApiError::invalid_request("relay payload is malformed"))?
    };
    for segment in batch.segments {
        state
            .repository_replica
            .lock()
            .await
            .receive_wire_from_repository(
                &state.cluster.cluster_id,
                &segment.identity,
                &segment.wire,
                u64::try_from(Utc::now().timestamp()).unwrap_or_default(),
                &ready_repository_ids,
                &state.cluster.node_id,
            )
            .map_err(repository_error)?;
    }
    Ok(Json(serde_json::json!({})))
}

pub(super) async fn admin_internal_acknowledge_history_repository_tombstones(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryTombstoneAcknowledgementRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sender = ensure_ready_repository_sender(&state, internal).await?;
    if request
        .acknowledgements
        .iter()
        .any(|acknowledgement| acknowledgement.repository_id() != sender)
    {
        return Err(ApiError::unauthorized(
            "repository tombstone acknowledgement sender does not match",
        ));
    }
    state
        .repository_replica
        .lock()
        .await
        .acknowledge_tombstones(&request.acknowledgements)
        .map_err(repository_error)?;
    Ok(Json(serde_json::json!({})))
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
    let (ready_repository_ids, peers) = worker::ready_repository_peers(&state)
        .await
        .unwrap_or_default();
    let local_is_ready = ready_repository_ids
        .iter()
        .any(|repository_id| repository_id == &state.cluster.node_id);
    let local_response = {
        let runtime = state.repository_replica.lock().await;
        if local_is_ready {
            runtime
                .query(
                    &state.cluster.node_id,
                    query.clone(),
                    LocalQueryMetadata::current_window(now),
                )
                .map_err(repository_error)?
        } else {
            runtime
                .query_local_only(query.clone(), LocalQueryMetadata::current_window(now))
                .map_err(repository_error)?
        }
    };
    let body = serde_json::to_vec(&RepositoryHistoryQuery {
        start_unix_seconds: request.start_unix_seconds,
        end_unix_seconds: request.end_unix_seconds,
        page_size: request.page_size,
    })
    .map_err(|error| ApiError::internal(error.to_string()))?;
    let mut responses = vec![local_response];
    for peer in peers
        .iter()
        .filter(|peer| peer.node_id != state.cluster.node_id)
        .take(MAX_REPAIR_REQUEST_IDS)
    {
        if let Ok(response) = worker::repository_direct_request::<RepositoryHistoryQueryResponse>(
            &state,
            peer,
            axum::http::Method::POST,
            "/api/admin/_internal/history-repository/query",
            body.clone(),
        )
        .await
        {
            responses.push(response);
        }
    }
    let mut candidates = Vec::with_capacity(responses.len());
    for response in &responses {
        let plan = response.plan();
        let Some(coverage) = plan.coverage().cloned() else {
            continue;
        };
        let candidate = match plan.repository_id() {
            Some(repository_id) => QueryCandidate::ready(
                repository_id,
                coverage,
                plan.watermarks().iter().cloned(),
                plan.gaps().iter().cloned(),
                plan.clock_skew_seconds(),
            ),
            None => QueryCandidate::local(
                coverage,
                plan.watermarks().iter().cloned(),
                plan.gaps().iter().cloned(),
                plan.clock_skew_seconds(),
            ),
        }
        .map_err(|error| ApiError::internal(error.to_string()))?;
        candidates.push(candidate);
    }
    let selected = QuerySelector::select(&query, candidates)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let response = match selected.repository_id() {
        Some(repository_id) => responses
            .into_iter()
            .find(|response| response.plan().repository_id() == Some(repository_id))
            .ok_or_else(|| ApiError::internal("selected repository response is unavailable"))?,
        None => responses
            .into_iter()
            .find(|response| response.plan().repository_id().is_none())
            .ok_or_else(|| ApiError::internal("local history response is unavailable"))?,
    };
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

async fn ensure_ready_repository_sender(
    state: &AppState,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<String, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let sender = verified.context.sender_id;
    if ready_repository_ids(state)
        .await?
        .iter()
        .any(|repository_id| repository_id == &sender)
    {
        Ok(sender)
    } else {
        Err(ApiError::unauthorized("repository sender is not ready"))
    }
}

async fn ready_repository_ids(state: &AppState) -> Result<Vec<String>, ApiError> {
    let store = state.store.lock().await;
    let Some(membership) = store.state().repository_membership.as_ref() else {
        return Err(ApiError::conflict(
            "history repository membership is not configured",
        ));
    };
    let ready = membership
        .ready_members()
        .map(|member| member.node_id().as_str().to_owned())
        .collect::<Vec<_>>();
    if ready.is_empty() {
        return Err(ApiError::conflict(
            "no ready history repository is available",
        ));
    }
    Ok(ready)
}
