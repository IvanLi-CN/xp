use std::time::Duration;

use axum::{
    Json,
    extract::{Extension, Path, Query},
    http::{HeaderMap, StatusCode},
};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    domain::Node,
    id::new_ulid_string,
    raft::types::raft_node_id_from_ulid,
    uptime_monitor::{MonitorTarget, Observation, ObservationOutcome},
    uptime_runtime::{
        DraftClusterTest, DraftClusterTestObserver, DraftClusterTestObserverUpdate,
        DraftClusterTestState, DraftTestCreateOutcome,
    },
};

use super::{
    ApiError, AppState, DraftClusterTestRequest, TestMonitorRequest, TestMonitorResponse,
    ad_hoc_token_fingerprint, now_unix_seconds, run_target_preflight, storage_error,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct DraftClusterTestForwardRequest {
    request: DraftClusterTestRequest,
    caller_fingerprint: String,
    idempotency_key_hash: Option<String>,
    snapshot_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct DraftClusterTestQuery {
    coordinator_node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InterruptedDraftClusterTest {
    run_id: String,
    coordinator_node_id: String,
    state: DraftClusterTestState,
    interrupted_at_unix_seconds: u64,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum DraftClusterTestStatus {
    Full(DraftClusterTest),
    Interrupted(InterruptedDraftClusterTest),
}

pub(super) async fn admin_create_monitor_draft_test(
    Extension(state): Extension<AppState>,
    headers: HeaderMap,
    Json(request): Json<DraftClusterTestRequest>,
) -> Result<(StatusCode, Json<DraftClusterTestStatus>), ApiError> {
    let caller_fingerprint = ad_hoc_token_fingerprint(&headers);
    let idempotency_key_hash = idempotency_key_hash(&headers)?;
    let snapshot_hash = draft_snapshot_hash(&request)?;
    let request_id = headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .map(ToOwned::to_owned)
        .unwrap_or_else(new_ulid_string);
    if !super::super::is_leader(&super::super::raft_metrics(&state)) {
        tracing::debug!(
            target: "service_monitor.draft_test",
            ingress_node_id = %state.cluster.node_id,
            request_id = %request_id,
            forward_outcome = "follower_forward",
            "forwarding draft cluster test create"
        );
        return forward_create(
            &state,
            request,
            caller_fingerprint,
            idempotency_key_hash,
            snapshot_hash,
            request_id,
        )
        .await;
    }
    create_local(
        state,
        request,
        caller_fingerprint,
        idempotency_key_hash,
        snapshot_hash,
        request_id,
    )
    .await
}

pub(crate) async fn admin_internal_create_monitor_draft_test(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<super::super::InternalSignatureAuth>>,
    Json(forward): Json<DraftClusterTestForwardRequest>,
) -> Result<(StatusCode, Json<DraftClusterTestStatus>), ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized(
            "draft test forwarding requires signed internal authentication",
        ));
    }
    if !super::super::is_leader(&super::super::raft_metrics(&state)) {
        return Err(ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "draft test coordinator is no longer leader",
        ));
    }
    create_local(
        state,
        forward.request,
        forward.caller_fingerprint,
        forward.idempotency_key_hash,
        forward.snapshot_hash,
        new_ulid_string(),
    )
    .await
}

async fn create_local(
    state: AppState,
    request: DraftClusterTestRequest,
    caller_fingerprint: String,
    idempotency_key_hash: Option<String>,
    snapshot_hash: String,
    request_id: String,
) -> Result<(StatusCode, Json<DraftClusterTestStatus>), ApiError> {
    request
        .target
        .validate()
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    request
        .observer_policy
        .validate()
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    let now = now_unix_seconds();
    let permit = state
        .uptime
        .acquire_ad_hoc(now, &caller_fingerprint)
        .await
        .ok_or_else(|| {
            ApiError::new(
                "draft_test_rate_limited",
                StatusCode::TOO_MANY_REQUESTS,
                "draft cluster test rate limit or concurrency limit reached",
            )
        })?;
    let all_nodes = {
        let store = state.store.lock().await;
        store
            .list_nodes()
            .into_iter()
            .map(|node| node.node_id)
            .collect::<Vec<_>>()
    };
    let observer_node_ids = request.observer_policy.resolve(&all_nodes);
    let run = DraftClusterTest {
        run_id: new_ulid_string(),
        target: request.target,
        observer_policy: request.observer_policy,
        observer_node_ids: observer_node_ids.clone(),
        coordinator_node_id: state.cluster.node_id.clone(),
        state: if observer_node_ids.is_empty() {
            DraftClusterTestState::Unsupported
        } else {
            DraftClusterTestState::Queued
        },
        created_at_unix_seconds: now,
        expires_at_unix_seconds: now.saturating_add(crate::uptime_runtime::DRAFT_TEST_TTL_SECONDS),
        observers: observer_node_ids
            .into_iter()
            .map(|node_id| DraftClusterTestObserver {
                node_id,
                state: DraftClusterTestState::Queued,
                latency_ms: None,
                status_code: None,
                error: None,
                started_at_unix_seconds: None,
                completed_at_unix_seconds: None,
            })
            .collect(),
        reason: None,
    };
    let outcome = state
        .uptime
        .create_draft_test_idempotent(
            &run,
            &caller_fingerprint,
            idempotency_key_hash.as_deref(),
            &snapshot_hash,
            now,
        )
        .await
        .map_err(storage_error)?;
    if let DraftTestCreateOutcome::Existing(existing) = outcome {
        tracing::debug!(
            target: "service_monitor.draft_test",
            coordinator_node_id = %existing.coordinator_node_id,
            run_id = %existing.run_id,
            request_id = %request_id,
            forward_outcome = "idempotency_reuse",
            "reused draft cluster test"
        );
        return Ok((
            StatusCode::ACCEPTED,
            Json(DraftClusterTestStatus::Full(existing)),
        ));
    }
    if matches!(outcome, DraftTestCreateOutcome::IdempotencyConflict) {
        return Err(ApiError::new(
            "idempotency_conflict",
            StatusCode::CONFLICT,
            "idempotency key was already used for a different draft test snapshot",
        ));
    }
    let task_state = state.clone();
    let task_run = run.clone();
    tokio::spawn(async move {
        let _permit = permit;
        execute_draft_cluster_test(task_state, task_run).await;
    });
    tracing::debug!(
        target: "service_monitor.draft_test",
        coordinator_node_id = %run.coordinator_node_id,
        run_id = %run.run_id,
        request_id = %request_id,
        forward_outcome = "created",
        "created draft cluster test"
    );
    Ok((
        StatusCode::ACCEPTED,
        Json(DraftClusterTestStatus::Full(run)),
    ))
}

pub(super) async fn admin_get_monitor_draft_test(
    Extension(state): Extension<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<DraftClusterTestQuery>,
) -> Result<Json<DraftClusterTestStatus>, ApiError> {
    let coordinator = query.coordinator_node_id;
    if coordinator
        .as_deref()
        .is_some_and(|id| id != state.cluster.node_id)
    {
        return forward_status(&state, &run_id, coordinator.as_deref().unwrap()).await;
    }
    local_status(&state, &run_id).await
}

pub(crate) async fn admin_internal_get_monitor_draft_test(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<super::super::InternalSignatureAuth>>,
    Path(run_id): Path<String>,
) -> Result<Json<DraftClusterTestStatus>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized(
            "draft test forwarding requires signed internal authentication",
        ));
    }
    local_status(&state, &run_id).await
}

async fn local_status(
    state: &AppState,
    run_id: &str,
) -> Result<Json<DraftClusterTestStatus>, ApiError> {
    let run = state
        .uptime
        .draft_test(run_id, now_unix_seconds())
        .await
        .map_err(storage_error)?
        .ok_or_else(|| ApiError::not_found("draft cluster test not found or expired"))?;
    if matches!(run.state, DraftClusterTestState::Interrupted) {
        return Ok(Json(DraftClusterTestStatus::Interrupted(
            interrupted_status(&run, run.reason.as_deref().unwrap_or("interrupted")),
        )));
    }
    Ok(Json(DraftClusterTestStatus::Full(run)))
}

async fn forward_create(
    state: &AppState,
    request: DraftClusterTestRequest,
    caller_fingerprint: String,
    idempotency_key_hash: Option<String>,
    snapshot_hash: String,
    request_id: String,
) -> Result<(StatusCode, Json<DraftClusterTestStatus>), ApiError> {
    let Some(node) = leader_node(state).await else {
        return Err(ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "no draft test coordinator is currently available",
        ));
    };
    if node.node_id == state.cluster.node_id {
        return Err(ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "draft test coordinator is not a remote leader",
        ));
    }
    let body = serde_json::to_vec(&DraftClusterTestForwardRequest {
        request,
        caller_fingerprint,
        idempotency_key_hash,
        snapshot_hash,
    })
    .map_err(|error| ApiError::internal(error.to_string()))?;
    let response = super::super::mesh::send_mesh_internal_request(
        state,
        &state.mesh_client,
        &node,
        axum::http::Method::POST,
        "/api/admin/_internal/monitor-draft-tests".to_owned(),
        body,
        Some("application/json".to_owned()),
        Duration::from_secs(12),
        false,
        request_id,
    )
    .await
    .map_err(|_| {
        ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "draft test coordinator is unavailable",
        )
    })?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = response
        .bytes()
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    if !status.is_success() {
        return Err(forward_error(status, &body));
    }
    let run = serde_json::from_slice::<DraftClusterTestStatus>(&body)
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok((status, Json(run)))
}

async fn forward_status(
    state: &AppState,
    run_id: &str,
    coordinator_node_id: &str,
) -> Result<Json<DraftClusterTestStatus>, ApiError> {
    let node = {
        let store = state.store.lock().await;
        store
            .list_nodes()
            .into_iter()
            .find(|node| node.node_id == coordinator_node_id)
    };
    let Some(node) = node else {
        return Ok(Json(DraftClusterTestStatus::Interrupted(
            interrupted_status_for_missing_coordinator(run_id, coordinator_node_id),
        )));
    };
    let response = super::super::mesh::send_mesh_internal_request(
        state,
        &state.mesh_client,
        &node,
        axum::http::Method::GET,
        format!("/api/admin/_internal/monitor-draft-tests/{run_id}"),
        Vec::new(),
        None,
        Duration::from_secs(8),
        false,
        new_ulid_string(),
    )
    .await;
    let Ok(response) = response else {
        return Ok(Json(DraftClusterTestStatus::Interrupted(
            interrupted_status_for_missing_coordinator(run_id, coordinator_node_id),
        )));
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = response
        .bytes()
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    if status == StatusCode::NOT_FOUND {
        return Err(ApiError::not_found("draft cluster test not found"));
    }
    if !status.is_success() {
        return Ok(Json(DraftClusterTestStatus::Interrupted(
            interrupted_status_for_missing_coordinator(run_id, coordinator_node_id),
        )));
    }
    serde_json::from_slice(&body)
        .map(Json)
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))
}

fn forward_error(status: StatusCode, body: &[u8]) -> ApiError {
    #[derive(Deserialize)]
    struct ErrorEnvelope {
        error: ErrorBody,
    }
    #[derive(Deserialize)]
    struct ErrorBody {
        code: String,
        message: String,
    }
    let Ok(envelope) = serde_json::from_slice::<ErrorEnvelope>(body) else {
        return ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "draft test coordinator returned an invalid response",
        );
    };
    let code = match envelope.error.code.as_str() {
        "draft_test_rate_limited" => "draft_test_rate_limited",
        "idempotency_conflict" => "idempotency_conflict",
        "invalid_request" => "invalid_request",
        "unauthorized" => "unauthorized",
        "leader_unavailable" => "leader_unavailable",
        _ => "leader_unavailable",
    };
    ApiError::new(code, status, envelope.error.message)
}

fn idempotency_key_hash(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::invalid_request("Idempotency-Key must be valid ASCII"))?;
    if value.is_empty() || value.len() > 256 {
        return Err(ApiError::invalid_request(
            "Idempotency-Key must contain 1 to 256 bytes",
        ));
    }
    Ok(Some(hex::encode(Sha256::digest(value.as_bytes()))))
}

fn draft_snapshot_hash(request: &DraftClusterTestRequest) -> Result<String, ApiError> {
    serde_json::to_vec(request)
        .map(|payload| hex::encode(Sha256::digest(payload)))
        .map_err(|error| ApiError::internal(error.to_string()))
}

async fn leader_node(state: &AppState) -> Option<Node> {
    let leader_id = super::super::raft_metrics(state).current_leader?;
    let store = state.store.lock().await;
    store.list_nodes().into_iter().find(|node| {
        raft_node_id_from_ulid(&node.node_id)
            .ok()
            .is_some_and(|node_id| node_id == leader_id)
    })
}

fn interrupted_status(run: &DraftClusterTest, reason: &str) -> InterruptedDraftClusterTest {
    InterruptedDraftClusterTest {
        run_id: run.run_id.clone(),
        coordinator_node_id: run.coordinator_node_id.clone(),
        state: DraftClusterTestState::Interrupted,
        interrupted_at_unix_seconds: now_unix_seconds(),
        reason: reason.to_owned(),
    }
}

fn interrupted_status_for_missing_coordinator(
    run_id: &str,
    coordinator_node_id: &str,
) -> InterruptedDraftClusterTest {
    InterruptedDraftClusterTest {
        run_id: run_id.to_owned(),
        coordinator_node_id: coordinator_node_id.to_owned(),
        state: DraftClusterTestState::Interrupted,
        interrupted_at_unix_seconds: now_unix_seconds(),
        reason: "coordinator_unavailable".to_owned(),
    }
}

async fn execute_draft_cluster_test(state: AppState, run: DraftClusterTest) {
    if !super::super::is_leader(&super::super::raft_metrics(&state)) {
        let _ = state
            .uptime
            .interrupt_draft_test(&run.run_id, "coordinator_lost")
            .await;
        return;
    }
    let tasks = run.observers.into_iter().map(|observer| {
        let state = state.clone();
        let run_id = run.run_id.clone();
        let target = run.target.clone();
        tokio::spawn(async move {
            let delay = draft_stagger_delay(&run_id, &observer.node_id);
            tokio::time::sleep(Duration::from_millis(delay)).await;
            if !super::super::is_leader(&super::super::raft_metrics(&state)) {
                let _ = state
                    .uptime
                    .interrupt_draft_test(&run_id, "coordinator_lost")
                    .await;
                return;
            }
            let now = now_unix_seconds();
            let _ = state
                .uptime
                .update_draft_test_observer(
                    &run_id,
                    DraftClusterTestObserverUpdate {
                        node_id: observer.node_id.clone(),
                        state: DraftClusterTestState::Running,
                        latency_ms: None,
                        status_code: None,
                        error: None,
                        observed_at_unix_seconds: now,
                    },
                )
                .await;
            let result = draft_test_observer(&state, &target, &observer.node_id, now).await;
            if !super::super::is_leader(&super::super::raft_metrics(&state)) {
                let _ = state
                    .uptime
                    .interrupt_draft_test(&run_id, "coordinator_lost")
                    .await;
                return;
            }
            let (test_state, latency_ms, status_code, error) = match result {
                Ok(observation) => {
                    let state = match observation.outcome {
                        ObservationOutcome::Success => DraftClusterTestState::Succeeded,
                        ObservationOutcome::Unsupported => DraftClusterTestState::Unsupported,
                        _ => DraftClusterTestState::Failed,
                    };
                    (
                        state,
                        observation.latency_ms,
                        observation.status_code,
                        observation.error.map(|error| format!("{error:?}")),
                    )
                }
                Err(error) => (DraftClusterTestState::Failed, None, None, Some(error)),
            };
            let _ = state
                .uptime
                .update_draft_test_observer(
                    &run_id,
                    DraftClusterTestObserverUpdate {
                        node_id: observer.node_id,
                        state: test_state,
                        latency_ms,
                        status_code,
                        error,
                        observed_at_unix_seconds: now_unix_seconds(),
                    },
                )
                .await;
        })
    });
    let _ = join_all(tasks).await;
}

async fn draft_test_observer(
    state: &AppState,
    target: &MonitorTarget,
    node_id: &str,
    now: u64,
) -> Result<Observation, String> {
    if node_id == state.cluster.node_id {
        return Ok(run_target_preflight(state, target.clone(), now).await);
    }
    let node = {
        let store = state.store.lock().await;
        store
            .list_nodes()
            .into_iter()
            .find(|node| node.node_id == node_id)
    }
    .ok_or_else(|| "observer node is no longer registered".to_owned())?;
    let request = TestMonitorRequest {
        target: target.clone(),
    };
    let body = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    let response = super::super::mesh::send_mesh_internal_request(
        state,
        &state.mesh_client,
        &node,
        axum::http::Method::POST,
        "/api/admin/_internal/monitors/test".to_owned(),
        body,
        Some("application/json".to_owned()),
        Duration::from_secs(12),
        false,
        new_ulid_string(),
    )
    .await
    .map_err(|error| format!("{error:?}"))?;
    let response = response
        .json::<TestMonitorResponse>()
        .await
        .map_err(|error| error.to_string())?;
    response
        .observations
        .into_iter()
        .next()
        .ok_or_else(|| "observer returned no result".to_owned())
}

fn draft_stagger_delay(run_id: &str, node_id: &str) -> u64 {
    let digest = Sha256::digest(format!("{run_id}:{node_id}").as_bytes());
    u64::from(u16::from_be_bytes([digest[0], digest[1]])) % 751
}

#[cfg(test)]
mod tests {
    use super::draft_stagger_delay;

    #[test]
    fn draft_cluster_test_stagger_is_deterministic_and_bounded() {
        let first = draft_stagger_delay("run-1", "observer-a");
        assert_eq!(first, draft_stagger_delay("run-1", "observer-a"));
        assert!(first <= 750);
        assert_ne!(first, draft_stagger_delay("run-1", "observer-b"));
    }
}
