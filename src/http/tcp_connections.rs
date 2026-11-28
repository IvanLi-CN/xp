use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use axum::{
    Extension, Json,
    extract::{Path, Query},
};

use crate::{
    domain::Node,
    state::{DeletedEndpointRegistry, JsonSnapshotStore},
    tcp_connection_history::{
        ConnectionClass, Point, PointSeries, Resolution, filter_deleted_subjects,
        parse_daily_payload, series_from_daily_payloads,
    },
    tcp_connection_usage::{
        TcpConnectionEndpointView, TcpConnectionUsageEndpointOption,
        TcpConnectionUsageEndpointSeries, TcpConnectionUsageWarning, TcpConnectionUsageWindow,
        TcpConnectionUsageWindowView, build_window_view as build_tcp_connection_window_view,
    },
};

use super::{
    ApiError, AppState, InternalSignatureAuth, history_repository, send_mesh_internal_read,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HistoryWindow {
    Hours24,
    Days7,
    Days30,
    Days90,
}

impl HistoryWindow {
    pub(super) fn parse(raw: Option<&str>) -> Result<Self, &'static str> {
        match raw.unwrap_or("24h") {
            "24h" => Ok(Self::Hours24),
            "7d" => Ok(Self::Days7),
            "30d" => Ok(Self::Days30),
            "90d" => Ok(Self::Days90),
            _ => Err("invalid window, expected 24h, 7d, 30d or 90d"),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Hours24 => "24h",
            Self::Days7 => "7d",
            Self::Days30 => "30d",
            Self::Days90 => "90d",
        }
    }

    pub(super) fn is_long_term(self) -> bool {
        matches!(self, Self::Days30 | Self::Days90)
    }

    pub(super) fn duration(self) -> Duration {
        match self {
            Self::Hours24 => Duration::hours(24),
            Self::Days7 => Duration::days(7),
            Self::Days30 => Duration::days(30),
            Self::Days90 => Duration::days(90),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestedResolution {
    Auto,
    Minute,
    FiveMinute,
    Daily,
}

impl RequestedResolution {
    pub(super) fn parse(raw: Option<&str>) -> Result<Self, &'static str> {
        match raw.unwrap_or("auto") {
            "auto" => Ok(Self::Auto),
            "1m" => Ok(Self::Minute),
            "5m" => Ok(Self::FiveMinute),
            "daily" => Ok(Self::Daily),
            _ => Err("invalid resolution, expected auto, 1m, 5m or daily"),
        }
    }

    pub(super) fn resolve(self, window: HistoryWindow) -> Option<Resolution> {
        match (self, window) {
            (Self::Minute, HistoryWindow::Hours24) => None,
            (Self::Minute, _) => Some(Resolution::FiveMinute),
            (Self::FiveMinute, HistoryWindow::Hours24) => Some(Resolution::FiveMinute),
            (Self::FiveMinute, _) => Some(Resolution::FiveMinute),
            (Self::Daily, _) => Some(Resolution::Daily),
            (Self::Auto, HistoryWindow::Hours24) => None,
            (Self::Auto, HistoryWindow::Days7) => Some(Resolution::FiveMinute),
            (Self::Auto, HistoryWindow::Days30 | HistoryWindow::Days90) => Some(Resolution::Daily),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SeriesPoint {
    pub(super) bucket_start: String,
    pub(super) min: u32,
    pub(super) avg: f64,
    pub(super) max: u32,
    pub(super) samples_expected: u32,
    pub(super) samples_actual: u32,
    pub(super) truncated: bool,
    pub(super) partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SeriesView {
    pub(super) approximate: bool,
    pub(super) points: Vec<SeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ClassSeriesView {
    pub(super) class: String,
    pub(super) approximate: bool,
    pub(super) points: Vec<SeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SubjectSeriesView {
    pub(super) class: String,
    pub(super) subject: String,
    pub(super) approximate: bool,
    pub(super) points: Vec<SeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DeletedSubject {
    pub(super) class: String,
    pub(super) subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct GapView {
    pub(super) start: String,
    pub(super) end: String,
    pub(super) reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PartialBucket {
    pub(super) bucket_start: String,
    pub(super) reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ConnectionHistoryView {
    pub(super) window: String,
    pub(super) window_start: String,
    pub(super) window_end: String,
    pub(super) resolution: String,
    pub(super) quality: String,
    pub(super) reasons: Vec<String>,
    pub(super) truncated: bool,
    pub(super) truncated_buckets: Vec<String>,
    pub(super) partial_buckets: Vec<PartialBucket>,
    pub(super) deleted_subjects: Vec<DeletedSubject>,
    pub(super) subjects_truncated: bool,
    pub(super) subjects_omitted: bool,
    pub(super) subjects_next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) watermark: Option<String>,
    pub(super) gaps: Vec<GapView>,
    pub(super) total: SeriesView,
    pub(super) classes: Vec<ClassSeriesView>,
    pub(super) subjects: Vec<SubjectSeriesView>,
}

fn point_view(point: &Point) -> SeriesPoint {
    SeriesPoint {
        bucket_start: point.bucket_start.to_rfc3339(),
        min: point.min,
        avg: point.avg,
        max: point.max,
        samples_expected: point.samples_expected,
        samples_actual: point.samples_actual,
        truncated: point.truncated,
        partial: point.partial,
    }
}

fn class_approximate(resolution: Option<Resolution>) -> bool {
    // Daily class extrema come from per-subject extrema; raw and 5m class sums are exact.
    resolution == Some(Resolution::Daily)
}

pub(super) struct HistoryRequest<'a> {
    pub(super) series: &'a PointSeries,
    pub(super) window: HistoryWindow,
    pub(super) resolution: Option<Resolution>,
    pub(super) deleted: &'a DeletedEndpointRegistry,
    pub(super) node_id: &'a str,
    pub(super) endpoint_filter: &'a [String],
    pub(super) subjects_limit: usize,
    pub(super) reasons: Vec<String>,
    pub(super) gaps: Vec<GapView>,
}

pub(super) fn build_history_view(request: HistoryRequest<'_>) -> ConnectionHistoryView {
    let HistoryRequest {
        series,
        window,
        resolution,
        deleted,
        node_id,
        endpoint_filter,
        subjects_limit,
        reasons,
        gaps,
    } = request;
    let deleted_subjects = deleted
        .iter()
        .filter(|(_, record)| record.node_id == node_id)
        .map(|(endpoint_id, _)| DeletedSubject {
            class: ConnectionClass::Inbound.as_str().to_string(),
            subject: endpoint_id.clone(),
        })
        .collect::<Vec<_>>();

    let filter = endpoint_filter
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let matches_filter = |subject: &str| filter.is_empty() || filter.contains(&subject);

    let total_points = series.total.iter().map(point_view).collect::<Vec<_>>();
    let classes = series
        .classes
        .iter()
        .map(|(class, points)| ClassSeriesView {
            class: class.as_str().to_string(),
            approximate: class_approximate(resolution),
            points: points.iter().map(point_view).collect(),
        })
        .collect::<Vec<_>>();

    let mut visible = series.clone();
    filter_deleted_subjects(&mut visible, &deleted_by_class(deleted, node_id));
    let mut subjects = Vec::new();
    for ((class, subject), points) in &visible.subjects {
        if *class == ConnectionClass::Inbound && !matches_filter(subject) {
            continue;
        }
        subjects.push(SubjectSeriesView {
            class: class.as_str().to_string(),
            subject: subject.clone(),
            approximate: false,
            points: points.iter().map(point_view).collect(),
        });
    }
    let subjects_truncated = subjects.len() > subjects_limit;
    subjects.truncate(subjects_limit);

    let truncated_buckets = total_points
        .iter()
        .filter(|point| point.truncated)
        .map(|point| point.bucket_start.clone())
        .collect::<Vec<_>>();
    let partial_buckets = total_points
        .iter()
        .filter(|point| point.partial)
        .map(|point| PartialBucket {
            bucket_start: point.bucket_start.clone(),
            reason: "partial_read".to_string(),
        })
        .collect::<Vec<_>>();

    let mut reasons = reasons;
    if !deleted_subjects.is_empty() && !reasons.iter().any(|item| item == "endpoint_deleted") {
        reasons.push("endpoint_deleted".to_string());
    }
    let quality = if reasons.iter().any(|item| item == "unsupported") {
        "unsupported"
    } else if !truncated_buckets.is_empty()
        || !partial_buckets.is_empty()
        || series.total.is_empty()
        || reasons.iter().any(|item| {
            matches!(
                item.as_str(),
                "repository_unavailable"
                    | "repository_unsupported"
                    | "target_node_unsupported"
                    | "legacy_inbound_only"
                    | "capture_suspended"
            )
        })
    {
        "partial"
    } else {
        "complete"
    };

    let watermark = series
        .total
        .last()
        .map(|point| point.bucket_start.to_rfc3339());

    ConnectionHistoryView {
        window: window.as_str().to_string(),
        window_start: series
            .total
            .first()
            .map(|point| point.bucket_start)
            .unwrap_or_else(|| {
                crate::tcp_connection_history::floor_minute(Utc::now()) - window.duration()
                    + Duration::minutes(1)
            })
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        window_end: series
            .total
            .last()
            .map(|point| point.bucket_start)
            .unwrap_or_else(|| crate::tcp_connection_history::floor_minute(Utc::now()))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        resolution: resolution
            .map(|value| value.as_str().to_string())
            .unwrap_or_else(|| "1m".to_string()),
        quality: quality.to_string(),
        reasons,
        truncated: !truncated_buckets.is_empty(),
        truncated_buckets,
        partial_buckets,
        deleted_subjects,
        subjects_truncated,
        subjects_omitted: false,
        subjects_next_cursor: None,
        watermark,
        gaps,
        total: SeriesView {
            approximate: false,
            points: total_points,
        },
        classes,
        subjects,
    }
}

pub(super) fn repository_series(payloads: &[Vec<u8>]) -> (PointSeries, bool) {
    let parsed = payloads
        .iter()
        .filter_map(|payload| parse_daily_payload(payload))
        .collect::<Vec<_>>();
    let omitted = parsed.iter().any(|payload| payload.subjects_omitted);
    (series_from_daily_payloads(&parsed), omitted)
}

fn deleted_by_class(
    registry: &DeletedEndpointRegistry,
    node_id: &str,
) -> BTreeMap<ConnectionClass, Vec<String>> {
    let mut deleted = BTreeMap::new();
    for (endpoint_id, record) in registry {
        if record.node_id == node_id {
            deleted
                .entry(ConnectionClass::Inbound)
                .or_insert_with(Vec::new)
                .push(endpoint_id.clone());
        }
    }
    deleted
}

pub(super) fn class_filter(series: &PointSeries, classes: &[String]) -> PointSeries {
    if classes.is_empty() {
        return series.clone();
    }
    let mut filtered = PointSeries {
        total: series.total.clone(),
        classes: BTreeMap::new(),
        subjects: BTreeMap::new(),
    };
    for (class, points) in &series.classes {
        if classes.iter().any(|item| item == class.as_str()) {
            filtered.classes.insert(*class, points.clone());
        }
    }
    for ((class, subject), points) in &series.subjects {
        if classes.iter().any(|item| item == class.as_str()) {
            filtered
                .subjects
                .insert((*class, subject.clone()), points.clone());
        }
    }
    filtered
}

/// Local history for 24h/1m and 7d/5m. The legacy report keeps the old field shapes.
pub(super) struct LocalHistoryRequest<'a> {
    pub(super) data_dir: &'a std::path::Path,
    pub(super) node_id: &'a str,
    pub(super) window: HistoryWindow,
    pub(super) resolution: RequestedResolution,
    pub(super) classes: &'a [String],
    pub(super) endpoint_filter: &'a [String],
    pub(super) subjects_limit: usize,
    pub(super) deleted: &'a DeletedEndpointRegistry,
}

pub(super) fn build_local_tcp_history(request: LocalHistoryRequest<'_>) -> ConnectionHistoryView {
    let LocalHistoryRequest {
        data_dir,
        node_id,
        window,
        resolution,
        classes,
        endpoint_filter,
        subjects_limit,
        deleted,
    } = request;
    let resolved = resolution.resolve(window);
    let series = match crate::tcp_connection_history::local_series(
        data_dir,
        node_id,
        window.duration(),
        resolved,
    ) {
        Ok(series) => series,
        Err(err) => {
            tracing::warn!(%err, "failed to read local tcp connection history");
            crate::tcp_connection_history::PointSeries::default()
        }
    };
    let series = class_filter(&series, classes);
    let mut view = build_history_view(HistoryRequest {
        series: &series,
        window,
        resolution: resolved,
        deleted,
        node_id,
        endpoint_filter,
        subjects_limit,
        reasons: Vec::new(),
        gaps: Vec::new(),
    });
    view.window = window.as_str().to_string();
    view
}

pub(super) fn build_tcp_connection_endpoint_views(
    store: &JsonSnapshotStore,
    node_id: &str,
) -> Vec<TcpConnectionEndpointView> {
    let mut endpoints = store
        .list_endpoints()
        .into_iter()
        // Deleted endpoints must disappear from the legacy fields too, not only from subjects.
        .filter(|endpoint| {
            endpoint.node_id == node_id
                && !store
                    .state()
                    .deleted_endpoints
                    .contains_key(&endpoint.endpoint_id)
        })
        .map(|endpoint| TcpConnectionEndpointView {
            node_id: endpoint.node_id,
            endpoint_id: endpoint.endpoint_id,
            endpoint_tag: endpoint.tag,
            port: endpoint.port,
        })
        .collect::<Vec<_>>();
    endpoints.sort_by(|left, right| {
        left.endpoint_tag
            .cmp(&right.endpoint_tag)
            .then_with(|| left.port.cmp(&right.port))
            .then_with(|| left.endpoint_id.cmp(&right.endpoint_id))
    });
    endpoints
}

pub(super) fn build_local_node_tcp_connections_report(
    store: &JsonSnapshotStore,
    node_id: &str,
    window: TcpConnectionUsageWindow,
) -> TcpConnectionUsageWindowView {
    let endpoints = build_tcp_connection_endpoint_views(store, node_id);
    let mut report = build_tcp_connection_window_view(
        store.tcp_connection_usage(),
        Utc::now(),
        window,
        &endpoints,
    );
    // The registry is authoritative for endpoint visibility across every response field.
    let allowed = endpoints
        .iter()
        .map(|endpoint| endpoint.endpoint_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    report
        .per_endpoint_series
        .retain(|series| allowed.contains(series.endpoint_id.as_str()));
    report
}

pub(super) fn admin_tcp_response(
    node: Node,
    _window: HistoryWindow,
    report: TcpConnectionUsageWindowView,
    history: ConnectionHistoryView,
) -> AdminNodeTcpConnectionsResponse {
    AdminNodeTcpConnectionsResponse {
        node,
        warnings: report.warnings,
        endpoints: report.endpoints,
        per_endpoint_series: report.per_endpoint_series,
        history,
    }
}

pub(super) async fn admin_internal_get_local_node_tcp_connections(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    Query(query): Query<TcpConnectionUsageQuery>,
) -> Result<Json<AdminInternalNodeTcpConnectionsLocalResponse>, ApiError> {
    if internal.is_none() {
        return Err(ApiError::unauthorized("internal auth required"));
    }
    let window =
        HistoryWindow::parse(query.window.as_deref()).map_err(ApiError::invalid_request)?;
    if window.is_long_term() {
        return Err(ApiError::invalid_request(
            "long-term windows must be queried from a repository",
        ));
    }
    let resolution = RequestedResolution::parse(query.resolution.as_deref())
        .map_err(ApiError::invalid_request)?;
    validate_connection_classes(&query.class)?;
    let subjects_limit = query.subjects_limit.unwrap_or(20).min(100);
    let node = {
        let store = state.store.lock().await;
        store.get_node(&state.cluster.node_id).ok_or_else(|| {
            ApiError::not_found(format!("node not found: {}", state.cluster.node_id))
        })?
    };
    let legacy_window = TcpConnectionUsageWindow::parse(query.window.as_deref())
        .map_err(ApiError::invalid_request)?;
    let (report, history) = {
        let store = state.store.lock().await;
        let report = build_local_node_tcp_connections_report(&store, &node.node_id, legacy_window);
        let history = build_local_tcp_history(LocalHistoryRequest {
            data_dir: &state.config.data_dir,
            node_id: &node.node_id,
            window,
            resolution,
            classes: &query.class,
            endpoint_filter: &query.endpoint_id,
            subjects_limit,
            deleted: &store.state().deleted_endpoints,
        });
        (report, history)
    };
    Ok(Json(AdminInternalNodeTcpConnectionsLocalResponse {
        node,
        warnings: report.warnings,
        endpoints: report.endpoints,
        per_endpoint_series: report.per_endpoint_series,
        history,
    }))
}

pub(super) async fn admin_get_node_tcp_connections(
    Extension(state): Extension<AppState>,
    Path(node_id): Path<String>,
    Query(query): Query<TcpConnectionUsageQuery>,
) -> Result<Json<AdminNodeTcpConnectionsResponse>, ApiError> {
    let window =
        HistoryWindow::parse(query.window.as_deref()).map_err(ApiError::invalid_request)?;
    let resolution = RequestedResolution::parse(query.resolution.as_deref())
        .map_err(ApiError::invalid_request)?;
    validate_connection_classes(&query.class)?;
    let subjects_limit = query.subjects_limit.unwrap_or(20).min(100);
    let node = {
        let store = state.store.lock().await;
        store
            .get_node(&node_id)
            .ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?
    };

    // 30d/90d come from a ready repository and never depend on the target node being online.
    if window.is_long_term() {
        return admin_get_node_tcp_connections_from_repository(
            &state,
            node,
            window,
            resolution,
            &query.class,
            &query.endpoint_id,
            subjects_limit,
        )
        .await;
    }

    let legacy_window = TcpConnectionUsageWindow::parse(query.window.as_deref())
        .map_err(ApiError::invalid_request)?;
    if node.node_id == state.cluster.node_id {
        let (report, history) = {
            let store = state.store.lock().await;
            let report =
                build_local_node_tcp_connections_report(&store, &node.node_id, legacy_window);
            let history = build_local_tcp_history(LocalHistoryRequest {
                data_dir: &state.config.data_dir,
                node_id: &node.node_id,
                window,
                resolution,
                classes: &query.class,
                endpoint_filter: &query.endpoint_id,
                subjects_limit,
                deleted: &store.state().deleted_endpoints,
            });
            (report, history)
        };
        return Ok(Json(admin_tcp_response(node, window, report, history)));
    }

    let base = node.api_base_url.trim_end_matches('/');
    if base.is_empty() {
        return Err(ApiError::internal(format!(
            "node is unreachable: {}",
            node.node_id
        )));
    }

    let client = state.mesh_client.clone();
    let response = send_mesh_internal_read(
        &state,
        &client,
        &node,
        format!(
            "/api/admin/_internal/nodes/tcp-connections/local?window={}",
            window.as_str()
        ),
        std::time::Duration::from_secs(3),
    )
    .await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // An older target node has no new history fields; degrade instead of fabricating them.
        let (report, history) = {
            let store = state.store.lock().await;
            let report = TcpConnectionUsageWindowView {
                window_start: String::new(),
                window_end: String::new(),
                warnings: Vec::new(),
                endpoints: Vec::new(),
                per_endpoint_series: Vec::new(),
            };
            let history = build_history_view(HistoryRequest {
                series: &crate::tcp_connection_history::PointSeries::default(),
                window,
                resolution: resolution.resolve(window),
                deleted: &store.state().deleted_endpoints,
                node_id: &node.node_id,
                endpoint_filter: &query.endpoint_id,
                subjects_limit,
                reasons: vec![
                    "target_node_unsupported".to_string(),
                    "unsupported".to_string(),
                ],
                gaps: Vec::new(),
            });
            (report, history)
        };
        return Ok(Json(admin_tcp_response(node, window, report, history)));
    }

    if !response.status().is_success() {
        return Err(ApiError::internal(format!(
            "node tcp connections request failed: {}",
            response.status()
        )));
    }

    let remote = response
        .json::<AdminInternalNodeTcpConnectionsLocalResponse>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(AdminNodeTcpConnectionsResponse {
        node: remote.node,
        warnings: remote.warnings,
        endpoints: remote.endpoints,
        per_endpoint_series: remote.per_endpoint_series,
        history: remote.history,
    }))
}

pub(super) fn validate_connection_classes(classes: &[String]) -> Result<(), ApiError> {
    for class in classes {
        if crate::tcp_connection_history::ConnectionClass::parse(class).is_none() {
            return Err(ApiError::invalid_request(format!(
                "invalid connection class: {class}"
            )));
        }
    }
    Ok(())
}

pub(super) async fn admin_get_node_tcp_connections_from_repository(
    state: &AppState,
    node: Node,
    window: HistoryWindow,
    resolution: RequestedResolution,
    classes: &[String],
    endpoint_filter: &[String],
    subjects_limit: usize,
) -> Result<Json<AdminNodeTcpConnectionsResponse>, ApiError> {
    let now = u64::try_from(Utc::now().timestamp()).unwrap_or_default();
    let start = now.saturating_sub(window.duration().num_seconds().max(0) as u64);
    let mut payloads = Vec::new();
    let mut reasons = Vec::new();
    let mut gaps = Vec::<GapView>::new();
    let mut page_cursor = None;
    for _ in 0..16 {
        let request = crate::http::history_repository::RepositoryHistoryQuery {
            start_unix_seconds: start,
            end_unix_seconds: now,
            page_size: 512,
            page_cursor: page_cursor.clone(),
            subject_node_id: Some(node.node_id.clone()),
            schema_id: Some(
                crate::tcp_connection_history::TCP_CONNECTION_HISTORY_SCHEMA.to_string(),
            ),
        };
        let response = match history_repository::query_history_repository(state, request).await {
            Ok(response) => response,
            Err(_) => {
                reasons.push("repository_unavailable".to_string());
                break;
            }
        };
        let plan = response.plan();
        if plan.repository_id().is_none() {
            reasons.push("repository_unsupported".to_string());
            break;
        }
        // Repository gaps are part of the managed response contract, not an empty placeholder.
        for gap in plan.gaps() {
            let (from, to) = (
                gap.range().start_unix_seconds(),
                gap.range().end_unix_seconds(),
            );
            let (Ok(from), Ok(to)) = (i64::try_from(from), i64::try_from(to)) else {
                continue;
            };
            let (Some(from), Some(to)) = (
                DateTime::from_timestamp(from, 0),
                DateTime::from_timestamp(to, 0),
            ) else {
                continue;
            };
            gaps.push(GapView {
                start: from.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                end: to.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                reason: gap.reason().unwrap_or("repository_gap").to_string(),
            });
        }
        for record in response.records() {
            if record.schema_id() == crate::tcp_connection_history::TCP_CONNECTION_HISTORY_SCHEMA {
                payloads.push(record.payload().to_vec());
            }
        }
        let Some(cursor) = response.next_page_cursor().map(ToOwned::to_owned) else {
            break;
        };
        page_cursor = Some(cursor);
    }

    let (series, subjects_omitted) = repository_series(&payloads);
    let series = class_filter(&series, classes);
    let store = state.store.lock().await;
    let mut history = build_history_view(HistoryRequest {
        series: &series,
        window,
        resolution: resolution.resolve(window),
        deleted: &store.state().deleted_endpoints,
        node_id: &node.node_id,
        endpoint_filter,
        subjects_limit,
        reasons,
        gaps,
    });
    history.subjects_omitted = subjects_omitted;
    let endpoints = build_tcp_connection_endpoint_views(&store, &node.node_id)
        .into_iter()
        .filter(|endpoint| {
            !store
                .state()
                .deleted_endpoints
                .contains_key(&endpoint.endpoint_id)
        })
        .map(|endpoint| TcpConnectionUsageEndpointOption {
            endpoint_id: endpoint.endpoint_id,
            endpoint_tag: endpoint.endpoint_tag,
            port: endpoint.port,
        })
        .collect::<Vec<_>>();
    drop(store);

    Ok(Json(admin_tcp_response(
        node,
        window,
        TcpConnectionUsageWindowView {
            window_start: history.window_start.clone(),
            window_end: history.window_end.clone(),
            warnings: Vec::new(),
            endpoints,
            per_endpoint_series: Vec::new(),
        },
        history,
    )))
}

#[derive(Debug, Deserialize)]
pub(super) struct TcpConnectionUsageQuery {
    window: Option<String>,
    resolution: Option<String>,
    #[serde(default)]
    class: Vec<String>,
    #[serde(default)]
    endpoint_id: Vec<String>,
    subjects_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct AdminNodeTcpConnectionsResponse {
    node: Node,
    warnings: Vec<TcpConnectionUsageWarning>,
    endpoints: Vec<TcpConnectionUsageEndpointOption>,
    per_endpoint_series: Vec<TcpConnectionUsageEndpointSeries>,
    #[serde(flatten)]
    history: ConnectionHistoryView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct AdminInternalNodeTcpConnectionsLocalResponse {
    node: Node,
    warnings: Vec<TcpConnectionUsageWarning>,
    endpoints: Vec<TcpConnectionUsageEndpointOption>,
    per_endpoint_series: Vec<TcpConnectionUsageEndpointSeries>,
    #[serde(flatten)]
    history: ConnectionHistoryView,
}
