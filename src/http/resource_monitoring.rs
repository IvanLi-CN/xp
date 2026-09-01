use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Extension, Path, Query},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use super::{
    ApiError, ApiJson, AppState, CLUSTER_RUNTIME_FANOUT_TIMEOUT, mesh::send_mesh_internal_read,
};
use crate::resource_monitoring::{
    ResourceGap, ResourceHistoryResponse, ResourcePolicy, ResourceRecentSeries, ResourceRole,
    ResourceSeriesPoint, ResourceSnapshot, unsupported_snapshot, validate_history_metric,
};
use crate::state::history_repository::replica::RepositoryHistoryQueryResponse;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct AdminNodesResourcesResponse {
    partial: bool,
    unreachable_nodes: Vec<String>,
    items: Vec<ResourceSnapshot>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResourceSeriesQuery {
    metric: String,
    role: Option<ResourceRole>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResourceHistoryQuery {
    metric: String,
    role: Option<ResourceRole>,
    from: Option<i64>,
    to: Option<i64>,
    resolution: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResourcePolicyUpdateRequest {
    expected_revision: u64,
    policy: ResourcePolicy,
}

pub(super) async fn admin_list_nodes_resources(
    Extension(state): Extension<AppState>,
) -> Result<Json<AdminNodesResourcesResponse>, ApiError> {
    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let mut items = Vec::new();
    let mut unreachable_nodes = Vec::new();
    for node in nodes {
        if node.node_id == state.cluster.node_id {
            items.push(state.resource_monitoring.current().await);
            continue;
        }
        let response = send_mesh_internal_read(
            &state,
            &state.mesh_client,
            &node,
            "/api/admin/_internal/nodes/resources/local".to_string(),
            CLUSTER_RUNTIME_FANOUT_TIMEOUT,
        )
        .await;
        match response {
            Ok(response) if response.status().is_success() => {
                match response.json::<ResourceSnapshot>().await {
                    Ok(snapshot) => items.push(snapshot),
                    Err(_) => unreachable_nodes.push(node.node_id),
                }
            }
            Ok(response) if response.status() == StatusCode::NOT_FOUND => {
                items.push(unsupported_snapshot(&node.node_id));
            }
            _ => unreachable_nodes.push(node.node_id),
        }
    }
    items.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    unreachable_nodes.sort();
    unreachable_nodes.dedup();
    Ok(Json(AdminNodesResourcesResponse {
        partial: !unreachable_nodes.is_empty(),
        unreachable_nodes,
        items,
    }))
}

pub(super) async fn admin_get_node_resources(
    Extension(state): Extension<AppState>,
    Path(node_id): Path<String>,
) -> Result<Json<ResourceSnapshot>, ApiError> {
    let node = {
        let store = state.store.lock().await;
        store
            .get_node(&node_id)
            .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?
    };
    if node.node_id == state.cluster.node_id {
        return Ok(Json(state.resource_monitoring.current().await));
    }
    let response = send_mesh_internal_read(
        &state,
        &state.mesh_client,
        &node,
        "/api/admin/_internal/nodes/resources/local".to_string(),
        CLUSTER_RUNTIME_FANOUT_TIMEOUT,
    )
    .await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(Json(unsupported_snapshot(&node_id)));
    }
    if !response.status().is_success() {
        return Err(ApiError::new(
            "resource_monitoring_unsupported",
            StatusCode::NOT_IMPLEMENTED,
            "node does not expose resource monitoring",
        ));
    }
    response
        .json::<ResourceSnapshot>()
        .await
        .map(Json)
        .map_err(|error| ApiError::internal(error.to_string()))
}

pub(super) async fn admin_get_node_resources_recent(
    Extension(state): Extension<AppState>,
    Path(node_id): Path<String>,
    Query(query): Query<ResourceSeriesQuery>,
) -> Result<Json<ResourceRecentSeries>, ApiError> {
    validate_history_metric(&query.metric, query.role).map_err(ApiError::invalid_request)?;
    let node = {
        let store = state.store.lock().await;
        store
            .get_node(&node_id)
            .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?
    };
    if node.node_id == state.cluster.node_id {
        return Ok(Json(
            state
                .resource_monitoring
                .recent(&query.metric, query.role)
                .await,
        ));
    }
    let mut path = format!(
        "/api/admin/_internal/nodes/resources/local/recent?metric={}",
        query.metric
    );
    if let Some(role) = query.role {
        path.push_str("&role=");
        path.push_str(role.as_str());
    }
    let response = send_mesh_internal_read(
        &state,
        &state.mesh_client,
        &node,
        path,
        CLUSTER_RUNTIME_FANOUT_TIMEOUT,
    )
    .await?;
    if !response.status().is_success() {
        return Err(ApiError::new(
            "resource_monitoring_unsupported",
            StatusCode::NOT_IMPLEMENTED,
            "node does not expose resource monitoring",
        ));
    }
    response
        .json::<ResourceRecentSeries>()
        .await
        .map(Json)
        .map_err(|error| ApiError::internal(error.to_string()))
}

pub(super) async fn admin_get_node_resources_history(
    Extension(state): Extension<AppState>,
    Path(node_id): Path<String>,
    Query(query): Query<ResourceHistoryQuery>,
) -> Result<Json<ResourceHistoryResponse>, ApiError> {
    validate_history_metric(&query.metric, query.role).map_err(ApiError::invalid_request)?;
    let node = {
        let store = state.store.lock().await;
        store
            .get_node(&node_id)
            .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?
    };
    let limit = query.limit.unwrap_or(1_500).clamp(1, 1_500);
    validate_resolution(query.resolution.as_deref())?;
    let now = chrono::Utc::now().timestamp().max(0) as u64;
    let start = query
        .from
        .unwrap_or_else(|| now.saturating_sub(365 * 24 * 60 * 60) as i64)
        .max(0) as u64;
    let end = query.to.unwrap_or(now as i64).max(0) as u64;
    if start > end {
        return Err(ApiError::invalid_request(
            "resource history from must be before to",
        ));
    }
    let selected_resolution = query
        .resolution
        .as_deref()
        .filter(|resolution| *resolution != "auto")
        .unwrap_or_else(|| auto_resolution_for_range(start, end));
    if let Ok(first_repository) = super::history_repository::query_resource_history_repository(
        &state,
        &node_id,
        start,
        end,
        limit.min(1_000),
    )
    .await
        && first_repository.plan().repository_id().is_some()
    {
        let mut repositories = vec![first_repository];
        for _ in 0..15 {
            let Some(cursor) = repositories
                .last()
                .and_then(RepositoryHistoryQueryResponse::next_page_cursor)
                .map(ToOwned::to_owned)
            else {
                break;
            };
            let page = super::history_repository::query_resource_history_repository_page(
                &state,
                &node_id,
                start,
                end,
                limit.min(1_000),
                Some(cursor),
            )
            .await?;
            let done = page.next_page_cursor().is_none();
            repositories.push(page);
            if done {
                break;
            }
        }
        return Ok(Json(resource_history_from_repository_pages(
            repositories,
            query.metric.clone(),
            query.role,
            selected_resolution,
            limit,
            start,
            end,
        )));
    }
    if node.node_id == state.cluster.node_id {
        return Ok(Json(state.resource_monitoring.history(
            query.metric,
            query.role,
            limit,
            query.from,
            query.to,
            query.resolution,
        )));
    }
    let mut path = format!(
        "/api/admin/_internal/nodes/resources/local/history?metric={}&limit={limit}",
        query.metric
    );
    if let Some(role) = query.role {
        path.push_str("&role=");
        path.push_str(role.as_str());
    }
    if let Some(from) = query.from {
        path.push_str(&format!("&from={from}"));
    }
    if let Some(to) = query.to {
        path.push_str(&format!("&to={to}"));
    }
    if let Some(resolution) = query.resolution {
        path.push_str("&resolution=");
        path.push_str(&resolution);
    }
    let response = send_mesh_internal_read(
        &state,
        &state.mesh_client,
        &node,
        path,
        CLUSTER_RUNTIME_FANOUT_TIMEOUT,
    )
    .await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(ApiError::new(
            "resource_monitoring_unsupported",
            StatusCode::NOT_IMPLEMENTED,
            "node does not expose resource monitoring",
        ));
    }
    if !response.status().is_success() {
        return Err(ApiError::new(
            "resource_history_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "resource history is unavailable",
        ));
    }
    response
        .json::<ResourceHistoryResponse>()
        .await
        .map(Json)
        .map_err(|error| ApiError::internal(error.to_string()))
}

pub(super) async fn admin_get_resource_policy(
    Extension(state): Extension<AppState>,
) -> Result<Json<ResourcePolicy>, ApiError> {
    Ok(Json(state.resource_monitoring.effective_policy().await))
}

pub(super) async fn admin_put_resource_policy(
    Extension(state): Extension<AppState>,
    ApiJson(request): ApiJson<ResourcePolicyUpdateRequest>,
) -> Result<Json<ResourcePolicy>, ApiError> {
    request
        .policy
        .validate()
        .map_err(ApiError::invalid_request)?;
    let mut policy = request.policy;
    policy.revision = request.expected_revision.saturating_add(1);
    super::raft_write(
        &state,
        crate::state::DesiredStateCommand::SetResourcePolicy {
            policy: policy.clone(),
            expected_revision: request.expected_revision,
        },
    )
    .await?;
    if let Err(error) = state.resource_monitoring.sync_policy(&policy) {
        tracing::warn!(
            ?error,
            "resource policy cache update failed after Raft commit"
        );
    }
    Ok(Json(policy))
}

pub(super) async fn admin_internal_get_local_node_resources(
    Extension(state): Extension<AppState>,
) -> Result<Json<ResourceSnapshot>, ApiError> {
    Ok(Json(state.resource_monitoring.current().await))
}

pub(super) async fn admin_internal_get_local_node_resources_recent(
    Extension(state): Extension<AppState>,
    Query(query): Query<ResourceSeriesQuery>,
) -> Result<Json<ResourceRecentSeries>, ApiError> {
    validate_history_metric(&query.metric, query.role).map_err(ApiError::invalid_request)?;
    Ok(Json(
        state
            .resource_monitoring
            .recent(&query.metric, query.role)
            .await,
    ))
}

pub(super) async fn admin_internal_get_local_node_resources_history(
    Extension(state): Extension<AppState>,
    Query(query): Query<ResourceHistoryQuery>,
) -> Result<Json<ResourceHistoryResponse>, ApiError> {
    validate_history_metric(&query.metric, query.role).map_err(ApiError::invalid_request)?;
    let limit = query.limit.unwrap_or(1_500).clamp(1, 1_500);
    validate_resolution(query.resolution.as_deref())?;
    Ok(Json(state.resource_monitoring.history(
        query.metric,
        query.role,
        limit,
        query.from,
        query.to,
        query.resolution,
    )))
}

fn validate_resolution(resolution: Option<&str>) -> Result<(), ApiError> {
    match resolution {
        None | Some("auto") | Some("1m") | Some("15m") | Some("1h") => Ok(()),
        Some(_) => Err(ApiError::invalid_request(
            "resource history resolution must be auto, 1m, 15m, or 1h",
        )),
    }
}

fn auto_resolution_for_range(start: u64, end: u64) -> &'static str {
    let span = end.saturating_sub(start);
    if span <= 14 * 24 * 60 * 60 {
        "1m"
    } else if span <= 104 * 24 * 60 * 60 {
        "15m"
    } else {
        "1h"
    }
}

fn resource_history_from_repository(
    response: RepositoryHistoryQueryResponse,
    metric: String,
    role: Option<ResourceRole>,
    resolution: &str,
    limit: usize,
    start: u64,
    end: u64,
) -> ResourceHistoryResponse {
    let key = role
        .map(|role| format!("{}.{}", role.as_str(), metric))
        .unwrap_or_else(|| format!("domain.{metric}"));
    let bucket_seconds = match resolution {
        "15m" => 15 * 60,
        "1h" => 60 * 60,
        _ => 60,
    };
    let mut aggregates = BTreeMap::<
        i64,
        (
            f64,
            u64,
            Option<f64>,
            crate::resource_monitoring::Capability,
        ),
    >::new();
    let mut gaps = response
        .plan()
        .gaps()
        .iter()
        .map(|gap| ResourceGap {
            from_bucket_start_unix_seconds: gap.range().start_unix_seconds() as i64,
            to_bucket_start_unix_seconds: gap.range().end_unix_seconds() as i64,
            reason_code: gap.reason().unwrap_or("repository_gap").to_string(),
        })
        .collect::<Vec<_>>();
    for record in response.records() {
        if record.schema_id() != crate::resource_monitoring::RESOURCE_HISTORY_SCHEMA {
            continue;
        }
        let Ok(payload) = serde_json::from_slice::<
            crate::resource_monitoring::ResourceHistoryPayload,
        >(record.payload()) else {
            continue;
        };
        match payload {
            crate::resource_monitoring::ResourceHistoryPayload::Rollup { rollup, .. } => {
                let Some(value) = rollup.values.get(&key) else {
                    continue;
                };
                let observed = rollup.bucket_start_unix_seconds;
                if observed < start as i64 || observed > end as i64 {
                    continue;
                }
                if let Some(metric_value) = value.mean.or(value.last) {
                    let bucket = observed.div_euclid(bucket_seconds) * bucket_seconds;
                    let entry = aggregates.entry(bucket).or_insert((
                        0.0,
                        0,
                        None,
                        crate::resource_monitoring::Capability::Supported,
                    ));
                    let weight = u64::from(rollup.captured_samples.max(1));
                    entry.0 += metric_value * weight as f64;
                    entry.1 = entry.1.saturating_add(weight);
                    entry.2 = Some(metric_value);
                    entry.3 = entry.3.max(value.capability).max(rollup.capability);
                }
            }
            crate::resource_monitoring::ResourceHistoryPayload::CaptureGap { gap, .. } => {
                gaps.push(gap);
            }
        }
    }
    let mut points = aggregates
        .into_iter()
        .filter_map(|(bucket, (sum, count, last, capability))| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(bucket, 0).map(|time| {
                ResourceSeriesPoint {
                    observed_at: time.to_rfc3339(),
                    value: (count > 0).then(|| sum / count as f64).or(last),
                    capability,
                }
            })
        })
        .collect::<Vec<_>>();
    let truncated = response.records_truncated() || points.len() > limit;
    if points.len() > limit {
        points.drain(..points.len() - limit);
    }
    gaps.sort_by_key(|gap| gap.from_bucket_start_unix_seconds);
    gaps.dedup_by(|left, right| {
        left.from_bucket_start_unix_seconds == right.from_bucket_start_unix_seconds
            && left.to_bucket_start_unix_seconds == right.to_bucket_start_unix_seconds
    });
    let coverage = points
        .first()
        .and_then(|point| {
            point
                .observed_at
                .parse::<chrono::DateTime<chrono::Utc>>()
                .ok()
        })
        .zip(points.last().and_then(|point| {
            point
                .observed_at
                .parse::<chrono::DateTime<chrono::Utc>>()
                .ok()
        }))
        .map(|(from, to)| (from.timestamp(), to.timestamp()));
    let watermark = coverage.map(|(_, to)| to);
    ResourceHistoryResponse {
        metric,
        role,
        resolution: resolution.to_string(),
        quality: if gaps.is_empty() {
            match response.plan().completeness() {
                crate::state::history_repository::query::Completeness::Complete => {
                    "complete".to_string()
                }
                crate::state::history_repository::query::Completeness::LocalOnly => {
                    "local_only".to_string()
                }
                crate::state::history_repository::query::Completeness::Partial => {
                    "partial".to_string()
                }
            }
        } else {
            "partial".to_string()
        },
        coverage,
        watermark,
        gaps,
        freshness_seconds: watermark
            .map(|value| chrono::Utc::now().timestamp().saturating_sub(value)),
        truncated,
        points,
    }
}

fn resource_history_from_repository_pages(
    responses: Vec<RepositoryHistoryQueryResponse>,
    metric: String,
    role: Option<ResourceRole>,
    resolution: &str,
    limit: usize,
    start: u64,
    end: u64,
) -> ResourceHistoryResponse {
    let mut pages = responses.into_iter();
    let Some(first) = pages.next() else {
        return ResourceHistoryResponse {
            metric,
            role,
            resolution: resolution.to_string(),
            quality: "local_only".to_string(),
            coverage: None,
            watermark: None,
            gaps: Vec::new(),
            freshness_seconds: None,
            truncated: false,
            points: Vec::new(),
        };
    };
    let mut result =
        resource_history_from_repository(first, metric, role, resolution, limit, start, end);
    for page in pages {
        let next = resource_history_from_repository(
            page,
            result.metric.clone(),
            result.role,
            resolution,
            limit,
            start,
            end,
        );
        result.points.extend(next.points);
        result.gaps.extend(next.gaps);
        result.truncated |= next.truncated;
        if next.quality == "partial" || next.quality == "local_only" {
            result.quality = next.quality;
        }
    }
    result
        .points
        .sort_by(|left, right| left.observed_at.cmp(&right.observed_at));
    result
        .points
        .dedup_by(|left, right| left.observed_at == right.observed_at);
    if result.points.len() > limit {
        result.points.drain(..result.points.len() - limit);
        result.truncated = true;
    }
    result
        .gaps
        .sort_by_key(|gap| gap.from_bucket_start_unix_seconds);
    result.gaps.dedup_by(|left, right| {
        left.from_bucket_start_unix_seconds == right.from_bucket_start_unix_seconds
            && left.to_bucket_start_unix_seconds == right.to_bucket_start_unix_seconds
    });
    result.quality = if result.gaps.is_empty() {
        result.quality
    } else {
        "partial".to_string()
    };
    result.coverage = result
        .points
        .first()
        .and_then(|point| {
            point
                .observed_at
                .parse::<chrono::DateTime<chrono::Utc>>()
                .ok()
        })
        .zip(result.points.last().and_then(|point| {
            point
                .observed_at
                .parse::<chrono::DateTime<chrono::Utc>>()
                .ok()
        }))
        .map(|(from, to)| (from.timestamp(), to.timestamp()));
    result.watermark = result.coverage.map(|(_, to)| to);
    result.freshness_seconds = result
        .watermark
        .map(|value| chrono::Utc::now().timestamp().saturating_sub(value));
    result
}
