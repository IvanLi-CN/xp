use std::{collections::BTreeMap, time::Duration};

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
    state::{DesiredStateCommand, history_repository::query::Completeness},
    uptime_monitor::{
        CurrentStatus, MAX_HISTORY_POINTS, MonitorKind, MonitorLifecycle, MonitorTarget,
        Observation, ObservationError, ObservationOutcome, ObservationRollup, ServiceMonitor,
        UPTIME_HISTORY_SCHEMA, UptimeHistoryPayload, next_slot, status_is_stale,
    },
    uptime_runtime::{AdHocRun, AdHocRunState, CaptureState},
};

use super::{ApiError, ApiJson, AppState, Items, raft_write};

mod history;

use history::{expected_slots, recent_history_summary, select_resolution, status_for};

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
            get(admin_get_service_monitor_status),
        )
        .route(
            "/monitors/{monitor_id}/history",
            get(admin_get_service_monitor_history),
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
    #[serde(default, deserialize_with = "deserialize_optional_observer_nodes")]
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

#[derive(Debug, Deserialize)]
pub(super) struct PatchMonitorRequest {
    expected_revision: u64,
    name: Option<String>,
    target: Option<MonitorTarget>,
    interval_seconds: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_optional_observer_nodes")]
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
        items.push(ServiceMonitorSummary {
            stale: status_is_stale(
                latest
                    .as_ref()
                    .map(|observation| observation.observed_at_unix_seconds),
                now,
                monitor.interval_seconds,
            ),
            status: status_for(latest.into_iter().collect(), capture),
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
        observer_node_ids: request.observer_node_ids.unwrap_or(None),
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
    if let Some(observer_node_ids) = request.observer_node_ids {
        replacement.observer_node_ids = observer_node_ids;
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
        let observation = uptime.run(&monitor, observer_node_id, now, true).await;
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
        observer_node_ids: Some(vec![state.cluster.node_id.clone()]),
        lifecycle: MonitorLifecycle::Active,
        revision: 1,
        revision_effective_at_unix_seconds: now,
    };
    state
        .uptime
        .run(&monitor, state.cluster.node_id.clone(), now, true)
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

pub(super) async fn admin_get_service_monitor_status(
    Extension(state): Extension<AppState>,
    Path(monitor_id): Path<String>,
) -> Result<Json<ServiceMonitorStatusResponse>, ApiError> {
    let monitor = monitor_for(&state, &monitor_id).await?;
    let now = now_unix_seconds();
    let capture = state.uptime.capture_state().await.map_err(storage_error)?;
    let quality = HistoryQuality::LocalOnly;
    let latest = state
        .uptime
        .latest(&monitor_id)
        .await
        .map_err(storage_error)?;
    let observer_node_ids = observer_node_ids(&state, &monitor).await;
    let observers = observer_node_ids
        .into_iter()
        .map(|node_id| {
            let local_observation = (node_id == state.cluster.node_id)
                .then(|| latest.clone())
                .flatten();
            ObserverStatus {
                state: status_for(local_observation.clone().into_iter().collect(), capture),
                latest: local_observation,
                icmp_supported: (node_id == state.cluster.node_id)
                    .then(crate::uptime_runtime::icmp_supported),
                node_id,
            }
        })
        .collect();
    Ok(Json(ServiceMonitorStatusResponse {
        freshness_seconds: latest
            .as_ref()
            .map(|observation| now.saturating_sub(observation.observed_at_unix_seconds)),
        stale: status_is_stale(
            latest
                .as_ref()
                .map(|observation| observation.observed_at_unix_seconds),
            now,
            monitor.interval_seconds,
        ),
        status: status_for(latest.into_iter().collect(), capture),
        monitor_id,
        capture,
        quality,
        observers,
    }))
}

pub(super) async fn admin_get_service_monitor_history(
    Extension(state): Extension<AppState>,
    Path(monitor_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ServiceMonitorHistoryResponse>, ApiError> {
    let monitor = monitor_for(&state, &monitor_id).await?;
    let now = now_unix_seconds();
    let end = query.to.unwrap_or(now).min(now);
    let start = query
        .from
        .unwrap_or_else(|| end.saturating_sub(24 * 60 * 60));
    if start > end {
        return Err(ApiError::invalid_request(
            "history from must not be after to",
        ));
    }
    let requested_limit = query.limit.unwrap_or(MAX_HISTORY_POINTS);
    let limit = requested_limit.clamp(1, MAX_HISTORY_POINTS);
    let (resolution, seconds) = select_resolution(query.resolution.as_deref(), start, end, limit)?;
    let repository =
        super::history_repository::query_service_monitor_history(&state, &monitor_id, start, end)
            .await?;
    let mut quality = match repository.plan().completeness() {
        Completeness::Complete => HistoryQuality::Complete,
        Completeness::Partial => HistoryQuality::Partial,
        Completeness::LocalOnly => HistoryQuality::LocalOnly,
    };
    let repository_selected = repository.plan().completeness() != Completeness::LocalOnly;
    let repository_truncated = repository.records_truncated();
    let mut buckets = BTreeMap::<u64, ObservationRollup>::new();
    if repository_selected {
        for record in repository.records() {
            if record.schema_id() != UPTIME_HISTORY_SCHEMA {
                continue;
            }
            let Ok(payload) = serde_json::from_slice::<UptimeHistoryPayload>(record.payload())
            else {
                continue;
            };
            if payload.ad_hoc
                || query
                    .observer_id
                    .as_deref()
                    .is_some_and(|observer_id| observer_id != payload.observer_node_id)
            {
                continue;
            }
            let observed_at = payload
                .bucket_start_unix_seconds
                .unwrap_or_else(|| record.observed_at_unix_seconds());
            let bucket_start = observed_at / seconds * seconds;
            buckets
                .entry(bucket_start)
                .or_default()
                .merge(&payload.rollup);
        }
    } else {
        let observations = state
            .uptime
            .observations(
                &monitor_id,
                start,
                end,
                MAX_HISTORY_POINTS.saturating_mul(32),
            )
            .await
            .map_err(storage_error)?;
        for observation in observations.into_iter().filter(|observation| {
            !observation.ad_hoc
                && query
                    .observer_id
                    .as_deref()
                    .is_none_or(|observer_id| observer_id == observation.observer_node_id)
        }) {
            let bucket_start = observation.observed_at_unix_seconds / seconds * seconds;
            let rollup = buckets.entry(bucket_start).or_default();
            rollup.record(&observation);
        }
    }
    let observer_count = if query.observer_id.is_some() {
        1
    } else {
        u64::try_from(observer_node_ids(&state, &monitor).await.len()).unwrap_or(u64::MAX)
    };
    let mut points = buckets
        .into_iter()
        .map(|(bucket_start, mut rollup)| {
            let bucket_end = bucket_start
                .saturating_add(seconds.saturating_sub(1))
                .min(end);
            rollup.expected = expected_slots(
                bucket_start.max(start),
                bucket_end,
                monitor.interval_seconds,
            )
            .saturating_mul(observer_count);
            HistoryPoint {
                start_unix_seconds: bucket_start,
                end_unix_seconds: bucket_end,
                availability_percent: rollup.availability_percent(),
                coverage_percent: rollup.coverage_percent(),
                rollup,
            }
        })
        .collect::<Vec<_>>();
    let truncated = repository_truncated || points.len() > limit;
    if truncated {
        points.drain(..points.len().saturating_sub(limit));
    }
    let expected = points
        .iter()
        .map(|point| point.rollup.expected)
        .sum::<u64>();
    let executed = points
        .iter()
        .map(|point| point.rollup.executed)
        .sum::<u64>();
    let gaps = points
        .iter()
        .filter(|point| point.rollup.executed < point.rollup.expected)
        .map(|point| HistoryGap {
            start_unix_seconds: point.start_unix_seconds,
            end_unix_seconds: point.end_unix_seconds,
            expected: point.rollup.expected,
            executed: point.rollup.executed,
        })
        .collect();
    if truncated {
        quality = HistoryQuality::Partial;
    }
    let latest = state
        .uptime
        .latest(&monitor_id)
        .await
        .map_err(storage_error)?;
    Ok(Json(ServiceMonitorHistoryResponse {
        monitor_id,
        resolution,
        coverage_percent: (expected > 0).then(|| executed as f64 * 100.0 / expected as f64),
        watermark_unix_seconds: latest
            .as_ref()
            .map(|observation| observation.observed_at_unix_seconds),
        freshness_seconds: latest
            .as_ref()
            .map(|observation| now.saturating_sub(observation.observed_at_unix_seconds)),
        quality,
        points,
        truncated,
        gaps,
        skew_seconds: 0,
    }))
}

async fn monitor_for(state: &AppState, monitor_id: &str) -> Result<ServiceMonitor, ApiError> {
    let store = state.store.lock().await;
    store
        .get_service_monitor(monitor_id)
        .ok_or_else(|| ApiError::not_found("service monitor not found"))
}

async fn observer_node_ids(state: &AppState, monitor: &ServiceMonitor) -> Vec<String> {
    if let Some(node_ids) = &monitor.observer_node_ids {
        return node_ids.clone();
    }
    let store = state.store.lock().await;
    store
        .list_nodes()
        .into_iter()
        .map(|node| node.node_id)
        .collect()
}

async fn ensure_observation_budget(
    state: &AppState,
    candidate: Option<&ServiceMonitor>,
    replacing_monitor_id: Option<&str>,
) -> Result<(), ApiError> {
    let (mut monitors, node_count) = {
        let store = state.store.lock().await;
        (store.list_service_monitors(), store.list_nodes().len())
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
            let observers = monitor
                .observer_node_ids
                .as_ref()
                .map_or(node_count, Vec::len);
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

fn ad_hoc_token_fingerprint(headers: &HeaderMap) -> String {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    hex::encode(Sha256::digest(authorization.as_bytes()))
}

fn deserialize_optional_observer_nodes<'de, D>(
    deserializer: D,
) -> Result<Option<Option<Vec<String>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<Vec<String>>::deserialize(deserializer)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_history_resolution_obeys_the_point_budget() {
        assert_eq!(
            select_resolution(None, 0, 1_499 * 60, MAX_HISTORY_POINTS).unwrap(),
            ("1m".to_owned(), 60)
        );
        assert_eq!(
            select_resolution(None, 0, 1_500 * 60, MAX_HISTORY_POINTS).unwrap(),
            ("5m".to_owned(), 300)
        );
        assert!(select_resolution(Some("daily"), 0, 1, 1).is_err());
    }

    #[test]
    fn expected_slots_include_the_first_aligned_slot() {
        assert_eq!(expected_slots(61, 180, 60), 2);
        assert_eq!(expected_slots(61, 119, 60), 0);
    }

    #[test]
    fn recent_summary_has_six_hours_of_five_minute_status_slots() {
        let monitor = ServiceMonitor {
            monitor_id: "01JMONITOR00000000000000000".to_owned(),
            name: "Public API".to_owned(),
            target: MonitorTarget::Ping {
                host: "example.net".to_owned(),
            },
            interval_seconds: 3_600,
            observer_node_ids: None,
            lifecycle: MonitorLifecycle::Active,
            revision: 1,
            revision_effective_at_unix_seconds: 0,
        };
        let success = Observation {
            monitor_id: monitor.monitor_id.clone(),
            revision: 1,
            observer_node_id: "01JNODE0000000000000000001".to_owned(),
            slot_unix_seconds: 68_400,
            observed_at_unix_seconds: 68_400,
            outcome: crate::uptime_monitor::ObservationOutcome::Success,
            error: None,
            latency_ms: Some(42),
            status_code: None,
            packet_loss_percent: 0,
            ad_hoc: false,
        };
        let failure = Observation {
            slot_unix_seconds: 86_400,
            observed_at_unix_seconds: 86_400,
            outcome: crate::uptime_monitor::ObservationOutcome::Failure,
            latency_ms: None,
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
