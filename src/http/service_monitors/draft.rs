use std::time::Duration;

use axum::{
    Json,
    extract::{Extension, Path},
    http::{HeaderMap, StatusCode},
};
use futures_util::future::join_all;
use sha2::{Digest as _, Sha256};

use crate::{
    id::new_ulid_string,
    uptime_monitor::{MonitorTarget, Observation, ObservationOutcome},
    uptime_runtime::{
        DraftClusterTest, DraftClusterTestObserver, DraftClusterTestObserverUpdate,
        DraftClusterTestState,
    },
};

use super::{
    ApiError, AppState, DraftClusterTestRequest, TestMonitorRequest, TestMonitorResponse,
    ad_hoc_token_fingerprint, now_unix_seconds, run_target_preflight, storage_error,
};

pub(super) async fn admin_create_monitor_draft_test(
    Extension(state): Extension<AppState>,
    headers: HeaderMap,
    Json(request): Json<DraftClusterTestRequest>,
) -> Result<(StatusCode, Json<DraftClusterTest>), ApiError> {
    request
        .target
        .validate()
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    request
        .observer_policy
        .validate()
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    let token_fingerprint = ad_hoc_token_fingerprint(&headers);
    let now = now_unix_seconds();
    let permit = state
        .uptime
        .acquire_ad_hoc(now, &token_fingerprint)
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
    };
    state
        .uptime
        .create_draft_test(&run)
        .await
        .map_err(storage_error)?;
    let task_state = state.clone();
    let task_run = run.clone();
    tokio::spawn(async move {
        let _permit = permit;
        execute_draft_cluster_test(task_state, task_run).await;
    });
    Ok((StatusCode::ACCEPTED, Json(run)))
}

pub(super) async fn admin_get_monitor_draft_test(
    Extension(state): Extension<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<DraftClusterTest>, ApiError> {
    state
        .uptime
        .draft_test(&run_id, now_unix_seconds())
        .await
        .map_err(storage_error)?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("draft cluster test not found or expired"))
}

async fn execute_draft_cluster_test(state: AppState, run: DraftClusterTest) {
    let tasks = run.observers.into_iter().map(|observer| {
        let state = state.clone();
        let run_id = run.run_id.clone();
        let target = run.target.clone();
        tokio::spawn(async move {
            let delay = draft_stagger_delay(&run_id, &observer.node_id);
            tokio::time::sleep(Duration::from_millis(delay)).await;
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
