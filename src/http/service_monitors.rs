use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Extension, Path, Query},
    http::{HeaderMap, StatusCode, header},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    id::new_ulid_string,
    state::DesiredStateCommand,
    uptime_monitor::{
        CurrentStatus, MAX_HISTORY_POINTS, MonitorKind, MonitorLifecycle, MonitorTarget,
        Observation, ObservationError, ObservationOutcome, ObservationRollup, ObserverPolicy,
        ObserverPolicyMode, ServiceMonitor, next_slot, status_is_stale,
    },
    uptime_runtime::{AdHocRun, AdHocRunState, CaptureState},
};

use super::{ApiError, ApiJson, AppState, Items, raft_write};

mod draft;
mod history;
mod status;

pub(super) use draft::{
    admin_internal_create_monitor_draft_test as draft_create,
    admin_internal_get_monitor_draft_test as draft_status,
};
use history::{recent_history_summary, status_for};

const OBSERVATION_BUDGET_PER_MINUTE: u64 = 300;
const RECENT_SUMMARY_BUCKET_SECONDS: u64 = 5 * 60;
const RECENT_SUMMARY_BUCKET_COUNT: usize = 6 * 12;

pub(super) fn router() -> Router {
    Router::new()
        .route(
            "/monitors",
            get(admin_list_service_monitors).post(admin_create_service_monitor),
        )
        .route("/monitors/test", post(admin_test_service_monitor))
        .route(
            "/monitor-draft-tests",
            post(draft::admin_create_monitor_draft_test),
        )
        .route(
            "/monitor-draft-tests/{run_id}",
            get(draft::admin_get_monitor_draft_test),
        )
        .route("/_internal/monitor-draft-tests", post(draft_create))
        .route("/_internal/monitor-draft-tests/{run_id}", get(draft_status))
        .route(
            "/monitors/{monitor_id}",
            get(admin_get_service_monitor)
                .patch(admin_patch_service_monitor)
                .delete(admin_delete_service_monitor),
        )
        .route(
            "/monitors/{monitor_id}/run",
            post(admin_run_service_monitor),
        )
        .route("/monitor-runs/{run_id}", get(admin_get_service_monitor_run))
        .route(
            "/monitors/{monitor_id}/status",
            get(status::admin_get_service_monitor_status),
        )
        .route(
            "/monitors/{monitor_id}/history",
            get(history::admin_get_service_monitor_history),
        )
}

#[derive(Debug, Deserialize)]
pub(super) struct MonitorListQuery {
    lifecycle: Option<MonitorLifecycle>,
    kind: Option<MonitorKind>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateMonitorRequest {
    name: String,
    target: MonitorTarget,
    #[serde(default)]
    interval_seconds: Option<u32>,
    #[serde(default)]
    observer_policy: Option<ObserverPolicy>,
    #[serde(default)]
    observer_node_ids: Option<Option<Vec<String>>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct TestMonitorRequest {
    target: MonitorTarget,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct TestMonitorResponse {
    target: MonitorTarget,
    observations: Vec<Observation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DraftClusterTestRequest {
    target: MonitorTarget,
    #[serde(default)]
    observer_policy: ObserverPolicy,
}

#[derive(Debug, Deserialize)]
pub(super) struct PatchMonitorRequest {
    expected_revision: u64,
    name: Option<String>,
    target: Option<MonitorTarget>,
    interval_seconds: Option<u32>,
    #[serde(default)]
    observer_policy: Option<ObserverPolicy>,
    #[serde(default)]
    observer_node_ids: Option<Option<Vec<String>>>,
    lifecycle: Option<MonitorLifecycle>,
}

#[derive(Debug, Deserialize)]
pub(super) struct DeleteMonitorQuery {
    expected_revision: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct HistoryQuery {
    #[serde(default)]
    from: Option<u64>,
    #[serde(default)]
    to: Option<u64>,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    observer_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum HistoryQuality {
    Complete,
    Partial,
    LocalOnly,
}

#[derive(Debug, Serialize)]
pub(super) struct ServiceMonitorSummary {
    #[serde(flatten)]
    monitor: ServiceMonitor,
    status: CurrentStatus,
    stale: bool,
    quality: HistoryQuality,
    recent_6h: RecentHistorySummary,
}

#[derive(Debug, Serialize)]
struct RecentHistorySummary {
    availability_percent: Option<f64>,
    coverage_percent: Option<f64>,
    expected: u64,
    executed: u64,
    latest_latency_ms: Option<u32>,
    latest_observed_at_unix_seconds: Option<u64>,
    slots: Vec<CurrentStatus>,
}

#[derive(Debug, Serialize)]
pub(super) struct ServiceMonitorStatusResponse {
    monitor_id: String,
    status: CurrentStatus,
    stale: bool,
    freshness_seconds: Option<u64>,
    capture: CaptureState,
    quality: HistoryQuality,
    observers: Vec<ObserverStatus>,
}

#[derive(Debug, Serialize)]
struct ObserverStatus {
    node_id: String,
    state: CurrentStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<Observation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icmp_supported: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(super) struct ServiceMonitorHistoryResponse {
    monitor_id: String,
    resolution: String,
    points: Vec<HistoryPoint>,
    truncated: bool,
    quality: HistoryQuality,
    coverage_percent: Option<f64>,
    watermark_unix_seconds: Option<u64>,
    gaps: Vec<HistoryGap>,
    skew_seconds: i64,
    freshness_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
struct HistoryPoint {
    start_unix_seconds: u64,
    end_unix_seconds: u64,
    rollup: ObservationRollup,
    availability_percent: Option<f64>,
    coverage_percent: Option<f64>,
}

#[derive(Debug, Serialize)]
struct HistoryGap {
    start_unix_seconds: u64,
    end_unix_seconds: u64,
    expected: u64,
    executed: u64,
}

#[derive(Debug, Serialize)]
pub(super) struct AdHocRunAccepted {
    run_id: String,
    state: AdHocRunState,
}

pub(super) async fn admin_list_service_monitors(
    Extension(state): Extension<AppState>,
    Query(query): Query<MonitorListQuery>,
) -> Result<Json<Items<ServiceMonitorSummary>>, ApiError> {
    let monitors = {
        let store = state.store.lock().await;
        store.list_service_monitors()
    };
    let now = now_unix_seconds();
    let capture = state.uptime.capture_state().await.map_err(storage_error)?;
    let quality = HistoryQuality::LocalOnly;
    let mut items = Vec::new();
    for monitor in monitors {
        if query
            .lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle != &monitor.lifecycle)
            || query
                .kind
                .as_ref()
                .is_some_and(|kind| kind != &monitor.target.kind())
        {
            continue;
        }
        let latest = state
            .uptime
            .latest(&monitor.monitor_id)
            .await
            .map_err(storage_error)?;
        let observations = state
            .uptime
            .observations(
                &monitor.monitor_id,
                now.saturating_sub(
                    RECENT_SUMMARY_BUCKET_SECONDS
                        .saturating_mul(RECENT_SUMMARY_BUCKET_COUNT as u64),
                ),
                now,
                MAX_HISTORY_POINTS.saturating_mul(32),
            )
            .await
            .map_err(storage_error)?;
        let recent_6h =
            recent_history_summary(&monitor, capture, now, latest.as_ref(), observations);
        let stale = status_is_stale(
            latest
                .as_ref()
                .map(|observation| observation.observed_at_unix_seconds),
            now,
            monitor.interval_seconds,
        );
        items.push(ServiceMonitorSummary {
            stale,
            status: status_for(latest.into_iter().collect(), capture, stale),
            monitor,
            quality,
            recent_6h,
        });
    }
    Ok(Json(Items { items }))
}

pub(super) async fn admin_create_service_monitor(
    Extension(state): Extension<AppState>,
    ApiJson(request): ApiJson<CreateMonitorRequest>,
) -> Result<(StatusCode, Json<ServiceMonitor>), ApiError> {
    let now = now_unix_seconds();
    let monitor = ServiceMonitor {
        monitor_id: new_ulid_string(),
        name: request.name,
        target: request.target,
        interval_seconds: request.interval_seconds.unwrap_or(60),
        observer_policy: request
            .observer_policy
            .or_else(|| legacy_observer_policy(request.observer_node_ids))
            .unwrap_or_default(),
        lifecycle: MonitorLifecycle::Active,
        revision: 1,
        revision_effective_at_unix_seconds: next_slot(now, request.interval_seconds.unwrap_or(60)),
    };
    ensure_observation_budget(&state, Some(&monitor), None).await?;
    raft_write(
        &state,
        DesiredStateCommand::CreateServiceMonitor {
            monitor: monitor.clone(),
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(monitor)))
}

pub(super) async fn admin_get_service_monitor(
    Extension(state): Extension<AppState>,
    Path(monitor_id): Path<String>,
) -> Result<Json<ServiceMonitor>, ApiError> {
    let store = state.store.lock().await;
    store
        .get_service_monitor(&monitor_id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("service monitor not found"))
}

pub(super) async fn admin_patch_service_monitor(
    Extension(state): Extension<AppState>,
    Path(monitor_id): Path<String>,
    ApiJson(request): ApiJson<PatchMonitorRequest>,
) -> Result<Json<ServiceMonitor>, ApiError> {
    let current = {
        let store = state.store.lock().await;
        store
            .get_service_monitor(&monitor_id)
            .ok_or_else(|| ApiError::not_found("service monitor not found"))?
    };
    if current.revision != request.expected_revision {
        return Err(revision_conflict());
    }
    if current.lifecycle == MonitorLifecycle::Deleted {
        return Err(ApiError::new(
            "monitor_deleted",
            StatusCode::CONFLICT,
            "deleted service monitors cannot be changed",
        ));
    }
    let lifecycle_only = request.lifecycle.is_some()
        && request.name.is_none()
        && request.target.is_none()
        && request.interval_seconds.is_none()
        && request.observer_policy.is_none()
        && request.observer_node_ids.is_none();
    if lifecycle_only {
        let lifecycle = request.lifecycle.expect("checked lifecycle");
        if lifecycle == MonitorLifecycle::Deleted {
            return Err(ApiError::invalid_request(
                "use DELETE to tombstone a service monitor",
            ));
        }
        let mut replacement = current.clone();
        replacement.lifecycle = lifecycle.clone();
        let replacement = current.next_revision(replacement, now_unix_seconds());
        ensure_observation_budget(&state, Some(&replacement), Some(&current.monitor_id)).await?;
        raft_write(
            &state,
            DesiredStateCommand::SetServiceMonitorLifecycle {
                monitor_id,
                lifecycle,
                expected_revision: request.expected_revision,
                revision_effective_at_unix_seconds: replacement.revision_effective_at_unix_seconds,
            },
        )
        .await?;
        let store = state.store.lock().await;
        return store
            .get_service_monitor(&current.monitor_id)
            .map(Json)
            .ok_or_else(|| ApiError::internal("service monitor was not committed"));
    }
    let mut replacement = current.clone();
    if let Some(name) = request.name {
        replacement.name = name;
    }
    if let Some(target) = request.target {
        replacement.target = target;
    }
    if let Some(interval_seconds) = request.interval_seconds {
        replacement.interval_seconds = interval_seconds;
    }
    if let Some(observer_policy) = request
        .observer_policy
        .or_else(|| legacy_observer_policy(request.observer_node_ids))
    {
        replacement.observer_policy = observer_policy;
    }
    if let Some(lifecycle) = request.lifecycle {
        if lifecycle == MonitorLifecycle::Deleted {
            return Err(ApiError::invalid_request(
                "use DELETE to tombstone a service monitor",
            ));
        }
        replacement.lifecycle = lifecycle;
    }
    let replacement = current.next_revision(replacement, now_unix_seconds());
    ensure_observation_budget(&state, Some(&replacement), Some(&current.monitor_id)).await?;
    raft_write(
        &state,
        DesiredStateCommand::UpdateServiceMonitor {
            monitor: replacement.clone(),
            expected_revision: request.expected_revision,
        },
    )
    .await?;
    Ok(Json(replacement))
}

pub(super) async fn admin_delete_service_monitor(
    Extension(state): Extension<AppState>,
    Path(monitor_id): Path<String>,
    Query(query): Query<DeleteMonitorQuery>,
) -> Result<StatusCode, ApiError> {
    raft_write(
        &state,
        DesiredStateCommand::DeleteServiceMonitor {
            monitor_id,
            expected_revision: query.expected_revision,
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn admin_run_service_monitor(
    Extension(state): Extension<AppState>,
    Path(monitor_id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AdHocRunAccepted>), ApiError> {
    let monitor = {
        let store = state.store.lock().await;
        store
            .get_service_monitor(&monitor_id)
            .ok_or_else(|| ApiError::not_found("service monitor not found"))?
    };
    if monitor.lifecycle == MonitorLifecycle::Deleted {
        return Err(ApiError::new(
            "monitor_deleted",
            StatusCode::CONFLICT,
            "deleted service monitors cannot be run",
        ));
    }
    let now = now_unix_seconds();
    let Some(permit) = state
        .uptime
        .acquire_ad_hoc(now, &ad_hoc_token_fingerprint(&headers))
        .await
    else {
        return Err(ApiError::new(
            "run_rate_limited",
            StatusCode::TOO_MANY_REQUESTS,
            "ad-hoc monitor run limit has been reached",
        ));
    };
    let run_id = new_ulid_string();
    state
        .uptime
        .create_ad_hoc_run(&AdHocRun {
            run_id: run_id.clone(),
            monitor_id: monitor.monitor_id.clone(),
            state: AdHocRunState::Queued,
            created_at_unix_seconds: now,
            completed_at_unix_seconds: None,
            observation: None,
            reason: None,
        })
        .await
        .map_err(storage_error)?;
    let uptime = state.uptime.clone();
    let observer_node_id = state.cluster.node_id.clone();
    let task_run_id = run_id.clone();
    tokio::spawn(async move {
        let _permit = permit;
        if uptime.mark_ad_hoc_run_running(&task_run_id).await.is_err() {
            return;
        }
        let observation = uptime
            .run(
                &monitor,
                observer_node_id.clone(),
                vec![observer_node_id],
                now,
                true,
            )
            .await;
        match uptime
            .record_with_id(task_run_id.clone(), observation.clone())
            .await
        {
            Ok(true) => {
                let _ = uptime.complete_ad_hoc_run(&task_run_id, &observation).await;
            }
            Ok(false) => {
                let _ = uptime
                    .reject_ad_hoc_run(&task_run_id, now_unix_seconds(), "capture_suspended")
                    .await;
            }
            Err(error) => {
                let _ = uptime
                    .reject_ad_hoc_run(&task_run_id, now_unix_seconds(), &error.to_string())
                    .await;
            }
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(AdHocRunAccepted {
            run_id,
            state: AdHocRunState::Queued,
        }),
    ))
}

pub(super) async fn admin_test_service_monitor(
    Extension(state): Extension<AppState>,
    Json(request): Json<TestMonitorRequest>,
) -> Result<Json<TestMonitorResponse>, ApiError> {
    request
        .target
        .validate()
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    let now = now_unix_seconds();
    let nodes = { state.store.lock().await.list_nodes() };
    let mut observations = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        if index > 0 {
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        if node.node_id == state.cluster.node_id {
            observations.push(run_target_preflight(&state, request.target.clone(), now).await);
            continue;
        }
        let body =
            serde_json::to_vec(&request).map_err(|error| ApiError::internal(error.to_string()))?;
        let response = super::mesh::send_mesh_internal_request(
            &state,
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
        .await;
        match response {
            Ok(response) => match response.json::<TestMonitorResponse>().await {
                Ok(remote) => observations.extend(remote.observations),
                Err(_) => observations.push(failed_target_preflight(&node.node_id, now)),
            },
            Err(_) => observations.push(failed_target_preflight(&node.node_id, now)),
        }
    }
    Ok(Json(TestMonitorResponse {
        target: request.target,
        observations,
    }))
}

fn failed_target_preflight(observer_node_id: &str, now: u64) -> Observation {
    Observation {
        monitor_id: format!("preflight-{}", new_ulid_string()),
        revision: 1,
        observer_node_id: observer_node_id.to_owned(),
        observer_set_node_ids: vec![observer_node_id.to_owned()],
        expected_observer_count: 1,
        slot_unix_seconds: now,
        observed_at_unix_seconds: now,
        outcome: ObservationOutcome::Failure,
        error: Some(ObservationError::Internal),
        latency_ms: None,
        status_code: None,
        packet_loss_percent: 0,
        ad_hoc: true,
    }
}

pub(super) async fn admin_internal_test_service_monitor(
    Extension(state): Extension<AppState>,
    Json(request): Json<TestMonitorRequest>,
) -> Result<Json<TestMonitorResponse>, ApiError> {
    request
        .target
        .validate()
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    let now = now_unix_seconds();
    let observation = run_target_preflight(&state, request.target.clone(), now).await;
    Ok(Json(TestMonitorResponse {
        target: request.target,
        observations: vec![observation],
    }))
}

async fn run_target_preflight(state: &AppState, target: MonitorTarget, now: u64) -> Observation {
    let monitor = ServiceMonitor {
        monitor_id: format!("preflight-{}", new_ulid_string()),
        name: "target preflight".to_owned(),
        target,
        interval_seconds: 60,
        observer_policy: ObserverPolicy {
            mode: ObserverPolicyMode::Include,
            node_ids: vec![state.cluster.node_id.clone()],
        },
        lifecycle: MonitorLifecycle::Active,
        revision: 1,
        revision_effective_at_unix_seconds: now,
    };
    state
        .uptime
        .run(
            &monitor,
            state.cluster.node_id.clone(),
            vec![state.cluster.node_id.clone()],
            now,
            true,
        )
        .await
}

pub(super) async fn admin_get_service_monitor_run(
    Extension(state): Extension<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<AdHocRun>, ApiError> {
    state
        .uptime
        .ad_hoc_run(&run_id)
        .await
        .map_err(storage_error)?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("ad-hoc monitor run not found"))
}

async fn monitor_for(state: &AppState, monitor_id: &str) -> Result<ServiceMonitor, ApiError> {
    let store = state.store.lock().await;
    store
        .get_service_monitor(monitor_id)
        .ok_or_else(|| ApiError::not_found("service monitor not found"))
}

async fn observer_node_ids(state: &AppState, monitor: &ServiceMonitor) -> Vec<String> {
    let store = state.store.lock().await;
    let all = store
        .list_nodes()
        .into_iter()
        .map(|node| node.node_id)
        .collect::<Vec<_>>();
    monitor.observer_policy.resolve(&all)
}

async fn ensure_observation_budget(
    state: &AppState,
    candidate: Option<&ServiceMonitor>,
    replacing_monitor_id: Option<&str>,
) -> Result<(), ApiError> {
    let (mut monitors, node_ids) = {
        let store = state.store.lock().await;
        (
            store.list_service_monitors(),
            store
                .list_nodes()
                .into_iter()
                .map(|node| node.node_id)
                .collect::<Vec<_>>(),
        )
    };
    if let Some(replacing_monitor_id) = replacing_monitor_id {
        monitors.retain(|monitor| monitor.monitor_id != replacing_monitor_id);
    }
    if let Some(candidate) = candidate {
        monitors.push(candidate.clone());
    }
    let slots_per_minute = monitors
        .iter()
        .filter(|monitor| monitor.lifecycle == MonitorLifecycle::Active)
        .map(|monitor| {
            let observers = monitor.observer_policy.resolve(&node_ids).len();
            u64::try_from(observers)
                .unwrap_or(u64::MAX)
                .saturating_mul(60)
                .div_ceil(u64::from(monitor.interval_seconds.max(1)))
        })
        .sum::<u64>();
    if slots_per_minute > OBSERVATION_BUDGET_PER_MINUTE {
        return Err(ApiError::new(
            "observation_budget_exceeded",
            StatusCode::UNPROCESSABLE_ENTITY,
            "service monitor observer slots exceed the cluster budget",
        ));
    }
    Ok(())
}

fn revision_conflict() -> ApiError {
    ApiError::new(
        "revision_conflict",
        StatusCode::CONFLICT,
        "service monitor changed since the edit started",
    )
}

fn storage_error(error: rusqlite::Error) -> ApiError {
    ApiError::internal(format!("uptime observation storage: {error}"))
}

fn now_unix_seconds() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default()
}

fn legacy_observer_policy(value: Option<Option<Vec<String>>>) -> Option<ObserverPolicy> {
    match value {
        Some(Some(node_ids)) if !node_ids.is_empty() => Some(ObserverPolicy {
            mode: ObserverPolicyMode::Include,
            node_ids,
        }),
        Some(_) => Some(ObserverPolicy::default()),
        None => None,
    }
}

fn ad_hoc_token_fingerprint(headers: &HeaderMap) -> String {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    hex::encode(Sha256::digest(authorization.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn auto_history_resolution_obeys_the_point_budget() {
        assert_eq!(
            history::select_resolution(None, 0, 1_499 * 60, MAX_HISTORY_POINTS).unwrap(),
            ("1m".to_owned(), 60)
        );
        assert_eq!(
            history::select_resolution(None, 0, 1_500 * 60, MAX_HISTORY_POINTS).unwrap(),
            ("5m".to_owned(), 300)
        );
        assert!(history::select_resolution(Some("daily"), 0, 1, 1).is_err());
    }

    #[test]
    fn expected_slots_include_the_first_aligned_slot() {
        assert_eq!(history::expected_slots(61, 180, 60), 2);
        assert_eq!(history::expected_slots(61, 119, 60), 0);
    }

    #[test]
    fn history_uses_the_observer_count_snapshotted_for_each_slot() {
        let expected_by_slot = BTreeMap::from([(60, 3), (120, 3), (180, 1)]);
        assert_eq!(
            history::expected_observations_in_bucket(
                &expected_by_slot,
                &Default::default(),
                60,
                180,
                false,
            ),
            7
        );
        assert_eq!(
            history::expected_observations_in_bucket(
                &expected_by_slot,
                &Default::default(),
                60,
                180,
                true,
            ),
            3
        );
    }

    #[test]
    fn keeps_the_latest_result_from_each_observer_for_cluster_status() {
        let mut latest_by_observer = BTreeMap::new();
        let mut tokyo = Observation {
            monitor_id: "monitor".to_owned(),
            revision: 1,
            observer_node_id: "tokyo".to_owned(),
            observer_set_node_ids: vec![
                "frankfurt".to_owned(),
                "singapore".to_owned(),
                "tokyo".to_owned(),
            ],
            expected_observer_count: 3,
            slot_unix_seconds: 60,
            observed_at_unix_seconds: 60,
            outcome: ObservationOutcome::Success,
            error: None,
            latency_ms: Some(xp_test_fixtures::number_value20()),
            status_code: Some(200),
            packet_loss_percent: 0,
            ad_hoc: false,
        };
        let mut singapore = tokyo.clone();
        singapore.observer_node_id = "singapore".to_owned();
        singapore.observed_at_unix_seconds = 90;
        singapore.outcome = ObservationOutcome::Failure;
        status::insert_latest_observation(&mut latest_by_observer, tokyo.clone());
        status::insert_latest_observation(&mut latest_by_observer, singapore.clone());
        tokyo.observed_at_unix_seconds = 30;
        tokyo.outcome = ObservationOutcome::Failure;
        status::insert_latest_observation(&mut latest_by_observer, tokyo);

        assert_eq!(latest_by_observer.len(), 2);
        assert_eq!(
            latest_by_observer.get("tokyo").unwrap().outcome,
            ObservationOutcome::Success
        );
        assert_eq!(
            latest_by_observer.get("singapore").unwrap().outcome,
            ObservationOutcome::Failure
        );
    }

    #[test]
    fn recent_summary_has_six_hours_of_five_minute_status_slots() {
        let monitor = ServiceMonitor {
            monitor_id: "01JMONITOR00000000000000000".to_owned(),
            name: "Public API".to_owned(),
            target: MonitorTarget::Ping {
                host: xp_test_fixtures::primary_host().to_owned(),
            },
            interval_seconds: 3_600,
            observer_policy: ObserverPolicy::default(),
            lifecycle: MonitorLifecycle::Active,
            revision: 1,
            revision_effective_at_unix_seconds: 0,
        };
        let success = Observation {
            monitor_id: monitor.monitor_id.clone(),
            revision: 1,
            observer_node_id: "01JNODE0000000000000000001".to_owned(),
            observer_set_node_ids: vec!["01JNODE0000000000000000001".to_owned()],
            expected_observer_count: 1,
            slot_unix_seconds: 68_400,
            observed_at_unix_seconds: 68_400,
            outcome: crate::uptime_monitor::ObservationOutcome::Success,
            error: None,
            latency_ms: Some(xp_test_fixtures::number_value42()),
            status_code: None,
            packet_loss_percent: 0,
            ad_hoc: false,
        };
        let failure = Observation {
            slot_unix_seconds: 86_400,
            observed_at_unix_seconds: 86_400,
            outcome: crate::uptime_monitor::ObservationOutcome::Failure,
            latency_ms: xp_test_fixtures::none(),
            ..success.clone()
        };

        let summary = recent_history_summary(
            &monitor,
            CaptureState {
                suspended: false,
                pending_observations: 0,
                pending_bytes: 0,
            },
            86_430,
            Some(&failure),
            vec![success, failure.clone()],
        );

        assert_eq!(summary.slots.len(), 72);
        assert_eq!(summary.expected, 6);
        assert_eq!(summary.executed, 2);
        assert_eq!(summary.availability_percent, Some(50.0));
        assert_eq!(summary.slots[11], CurrentStatus::Up);
        assert_eq!(summary.slots[71], CurrentStatus::Down);
        assert_eq!(summary.latest_latency_ms, None);
    }
}
