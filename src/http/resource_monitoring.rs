use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Extension, Path, Query},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use super::{
    ApiError, ApiJson, AppState, CLUSTER_RUNTIME_FANOUT_TIMEOUT,
    mesh::{
        MeshCapabilityProbeResponse, send_mesh_internal_capability_read,
        send_mesh_internal_resource_read,
    },
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
        let response = send_mesh_internal_resource_read(
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
                match classify_resource_response(
                    response.status(),
                    Some(resource_capability_status(&state, &node).await),
                ) {
                    ResourceResponseDisposition::Unsupported => {
                        items.push(unsupported_snapshot(&node.node_id));
                    }
                    ResourceResponseDisposition::Remote(_) => {
                        unreachable_nodes.push(node.node_id);
                    }
                    ResourceResponseDisposition::Success => unreachable_nodes.push(node.node_id),
                }
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
    let response = send_mesh_internal_resource_read(
        &state,
        &state.mesh_client,
        &node,
        "/api/admin/_internal/nodes/resources/local".to_string(),
        CLUSTER_RUNTIME_FANOUT_TIMEOUT,
    )
    .await?;
    let capability_status = if response.status() == StatusCode::NOT_FOUND {
        Some(resource_capability_status(&state, &node).await)
    } else {
        None
    };
    match classify_resource_response(response.status(), capability_status) {
        ResourceResponseDisposition::Unsupported => {
            return Ok(Json(unsupported_snapshot(&node_id)));
        }
        ResourceResponseDisposition::Remote(status) => {
            return Err(remote_resource_error(&node_id, status));
        }
        ResourceResponseDisposition::Success => {}
    }
    response
        .json::<ResourceSnapshot>()
        .await
        .map(Json)
        .map_err(|_| malformed_resource_response_error(&node_id))
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
    let response = send_mesh_internal_resource_read(
        &state,
        &state.mesh_client,
        &node,
        path,
        CLUSTER_RUNTIME_FANOUT_TIMEOUT,
    )
    .await?;
    let capability_status = if response.status() == StatusCode::NOT_FOUND {
        Some(resource_capability_status(&state, &node).await)
    } else {
        None
    };
    match classify_resource_response(response.status(), capability_status) {
        ResourceResponseDisposition::Unsupported => {
            return Ok(Json(unsupported_recent_series(&query.metric, query.role)));
        }
        ResourceResponseDisposition::Remote(status) => {
            return Err(remote_resource_error(&node_id, status));
        }
        ResourceResponseDisposition::Success => {}
    }
    response
        .json::<ResourceRecentSeries>()
        .await
        .map(Json)
        .map_err(|_| malformed_resource_response_error(&node_id))
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
    let response = send_mesh_internal_resource_read(
        &state,
        &state.mesh_client,
        &node,
        path,
        CLUSTER_RUNTIME_FANOUT_TIMEOUT,
    )
    .await?;
    let capability_status = if response.status() == StatusCode::NOT_FOUND {
        Some(resource_capability_status(&state, &node).await)
    } else {
        None
    };
    match classify_resource_response(response.status(), capability_status) {
        ResourceResponseDisposition::Unsupported => {
            return Err(ApiError::new(
                "resource_monitoring_unsupported",
                StatusCode::NOT_IMPLEMENTED,
                "node does not expose resource monitoring",
            ));
        }
        ResourceResponseDisposition::Remote(status) => {
            return Err(remote_resource_error(&node_id, status));
        }
        ResourceResponseDisposition::Success => {}
    }
    response
        .json::<ResourceHistoryResponse>()
        .await
        .map(Json)
        .map_err(|_| malformed_resource_response_error(&node_id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceCapabilityStatus {
    Supported,
    Unsupported,
    Unknown,
}

#[derive(Debug, Deserialize)]
struct InternalCapabilitiesResponse {
    capabilities: Vec<String>,
}

async fn resource_capability_status(
    state: &AppState,
    node: &crate::domain::Node,
) -> ResourceCapabilityStatus {
    let probe = send_mesh_internal_capability_read(
        state,
        &state.mesh_client,
        node,
        CLUSTER_RUNTIME_FANOUT_TIMEOUT,
    )
    .await;
    let Ok(probe) = probe else {
        return ResourceCapabilityStatus::Unknown;
    };
    let MeshCapabilityProbeResponse::Verified { response, deadline } = probe else {
        return ResourceCapabilityStatus::Unsupported;
    };
    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return classify_resource_capability(status, None);
    }
    if !status.is_success() {
        return classify_resource_capability(status, None);
    }
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    let Ok(body) = super::bounded_json::read_bounded_internal_json::<InternalCapabilitiesResponse>(
        response, remaining,
    )
    .await
    else {
        return ResourceCapabilityStatus::Unknown;
    };
    classify_resource_capability(status, Some(&body.capabilities))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceResponseDisposition {
    Success,
    Unsupported,
    Remote(StatusCode),
}

fn classify_resource_response(
    status: StatusCode,
    capability_status: Option<ResourceCapabilityStatus>,
) -> ResourceResponseDisposition {
    if status == StatusCode::NOT_FOUND
        && matches!(
            capability_status,
            Some(ResourceCapabilityStatus::Unsupported)
        )
    {
        return ResourceResponseDisposition::Unsupported;
    }
    if status.is_success() {
        ResourceResponseDisposition::Success
    } else {
        ResourceResponseDisposition::Remote(status)
    }
}

fn classify_resource_capability(
    status: StatusCode,
    capabilities: Option<&[String]>,
) -> ResourceCapabilityStatus {
    if status == StatusCode::NOT_FOUND {
        return ResourceCapabilityStatus::Unsupported;
    }
    if !status.is_success() {
        return ResourceCapabilityStatus::Unknown;
    }
    let Some(capabilities) = capabilities else {
        return ResourceCapabilityStatus::Unknown;
    };
    if capabilities
        .iter()
        .any(|capability| capability == "admin.resource-monitoring")
    {
        ResourceCapabilityStatus::Supported
    } else {
        ResourceCapabilityStatus::Unsupported
    }
}

fn remote_resource_error(node_id: &str, target_status: StatusCode) -> ApiError {
    ApiError::new(
        "remote_node_error",
        target_status,
        "the target node returned a resource error",
    )
    .with_detail("failure_layer", "remote_node")
    .with_detail("cause", "remote_resource_error")
    .with_detail("confidence", "confirmed")
    .with_detail("target_node_id", node_id)
    .with_detail("attempted_path", "mesh")
    .with_detail("dispatch_state", "verified_remote_response")
    .with_detail(
        "retryable",
        target_status == StatusCode::TOO_MANY_REQUESTS || target_status.is_server_error(),
    )
    .with_detail("target_status", target_status.as_u16())
    .with_detail("support_id", crate::id::new_ulid_string())
}

fn malformed_resource_response_error(node_id: &str) -> ApiError {
    ApiError::new(
        "peer_protocol_rejected",
        StatusCode::BAD_GATEWAY,
        "peer resource response could not be decoded",
    )
    .with_detail("failure_layer", "peer_protocol")
    .with_detail("cause", "protocol_rejected")
    .with_detail("confidence", "confirmed")
    .with_detail("target_node_id", node_id)
    .with_detail("attempted_path", "unknown")
    .with_detail("dispatch_state", "dispatched_no_verified_response")
    .with_detail("retryable", true)
    .with_detail("support_id", crate::id::new_ulid_string())
}

fn unsupported_recent_series(metric: &str, role: Option<ResourceRole>) -> ResourceRecentSeries {
    ResourceRecentSeries {
        metric: metric.to_string(),
        role,
        resolution: "15s".to_string(),
        points: Vec::new(),
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::{
        ResourceCapabilityStatus, ResourceResponseDisposition, classify_resource_capability,
        classify_resource_response, remote_resource_error,
    };

    #[test]
    fn resource_404_is_unsupported_only_after_capability_probe() {
        assert_eq!(
            classify_resource_response(
                StatusCode::NOT_FOUND,
                Some(ResourceCapabilityStatus::Unsupported),
            ),
            ResourceResponseDisposition::Unsupported
        );
        assert_eq!(
            classify_resource_response(
                StatusCode::NOT_FOUND,
                Some(ResourceCapabilityStatus::Supported),
            ),
            ResourceResponseDisposition::Remote(StatusCode::NOT_FOUND)
        );
        assert_eq!(
            classify_resource_response(
                StatusCode::NOT_FOUND,
                Some(ResourceCapabilityStatus::Unknown),
            ),
            ResourceResponseDisposition::Remote(StatusCode::NOT_FOUND)
        );
    }

    #[test]
    fn capability_route_404_is_legacy_unsupported_but_remote_404_is_preserved() {
        assert_eq!(
            classify_resource_capability(StatusCode::NOT_FOUND, None),
            ResourceCapabilityStatus::Unsupported
        );
        assert_eq!(
            classify_resource_capability(
                StatusCode::OK,
                Some(&["admin.resource-monitoring".to_string()]),
            ),
            ResourceCapabilityStatus::Supported
        );
        assert_eq!(
            classify_resource_capability(StatusCode::OK, None),
            ResourceCapabilityStatus::Unknown
        );
        assert_eq!(
            classify_resource_capability(StatusCode::INTERNAL_SERVER_ERROR, None),
            ResourceCapabilityStatus::Unknown
        );

        let remote = remote_resource_error("node-a", StatusCode::NOT_FOUND);
        assert_eq!(remote.code, "remote_node_error");
        assert_eq!(remote.status, StatusCode::NOT_FOUND);
        assert_eq!(remote.details["failure_layer"], "remote_node");
        assert_eq!(remote.details["target_status"], 404);
    }

    #[test]
    fn malformed_verified_resource_payload_is_safe_protocol_error() {
        let error = super::malformed_resource_response_error("node-a");
        assert_eq!(error.code, "peer_protocol_rejected");
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(error.message, "peer resource response could not be decoded");
        assert_eq!(error.details["failure_layer"], "peer_protocol");
        assert_eq!(
            error.details["dispatch_state"],
            "dispatched_no_verified_response"
        );
        assert_eq!(error.details["target_node_id"], "node-a");
        assert!(error.details["support_id"].as_str().is_some());
    }
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
