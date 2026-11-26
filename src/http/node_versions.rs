use axum::{Json, extract::Extension};
use futures_util::{StreamExt as _, stream};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::{ApiError, AppState, CLUSTER_RUNTIME_FANOUT_TIMEOUT, bounded_json};

#[derive(Debug, Clone, Deserialize)]
struct ClusterInfoVersionResponse {
    cluster_id: String,
    node_id: String,
    xp_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminNodeVersionItem {
    node_id: String,
    xp_version: Option<String>,
    error: Option<String>,
    /// True when the version came from the node's persisted signed self-report because a live
    /// query did not answer. It is evidence, not a live observation.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    reported: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AdminNodeVersionsResponse {
    items: Vec<AdminNodeVersionItem>,
}

pub(super) async fn admin_list_node_versions(
    Extension(state): Extension<AppState>,
) -> Result<Json<AdminNodeVersionsResponse>, ApiError> {
    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let local_node_id = state.cluster.node_id.clone();
    let cluster_id = state.cluster.cluster_id.clone();
    let client = state.mesh_client.clone();
    let fanout_deadline = Instant::now() + CLUSTER_RUNTIME_FANOUT_TIMEOUT;

    let store_for_items = state.store.clone();
    let items = stream::iter(nodes.into_iter().map(|node| {
        let client = client.clone();
        let store = store_for_items.clone();
        let cluster_id = cluster_id.clone();
        let local_node_id = local_node_id.clone();
        async move {
            if node.node_id == local_node_id {
                return AdminNodeVersionItem {
                    node_id: node.node_id,
                    xp_version: Some(crate::version::VERSION.to_string()),
                    error: None,
                    reported: false,
                };
            }

            let remaining = fanout_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return with_reported_fallback(node.node_id, "timeout", store.clone()).await;
            }

            let (response, deadline) = match client
                .send_registered_public_read(&node.api_base_url, "/api/cluster/info", remaining)
                .await
            {
                Ok(response) => response,
                Err(_) => {
                    return with_reported_fallback(node.node_id, "unreachable", store.clone())
                        .await;
                }
            };

            if !response.status().is_success() {
                return with_reported_fallback(
                    node.node_id,
                    &format!("http_{}", response.status().as_u16()),
                    store.clone(),
                )
                .await;
            }

            let body = match bounded_json::read_bounded_internal_json::<ClusterInfoVersionResponse>(
                response,
                deadline.saturating_duration_since(std::time::Instant::now()),
            )
            .await
            {
                Ok(body) => body,
                Err(_) => {
                    return with_reported_fallback(node.node_id, "invalid_response", store.clone())
                        .await;
                }
            };

            if body.cluster_id != cluster_id || body.node_id != node.node_id {
                return with_reported_fallback(node.node_id, "cluster_mismatch", store.clone())
                    .await;
            }

            AdminNodeVersionItem {
                node_id: node.node_id,
                xp_version: Some(body.xp_version),
                error: None,
                reported: false,
            }
        }
    }))
    .buffer_unordered(4)
    .collect::<Vec<_>>()
    .await;

    let mut items = items;
    items.sort_by(|left, right| left.node_id.cmp(&right.node_id));

    Ok(Json(AdminNodeVersionsResponse { items }))
}

/// Fall back to the node's persisted signed self-report when a live query did not answer. The
/// answer stays explicitly marked as reported so callers never mistake it for a live observation.
async fn with_reported_fallback(
    node_id: String,
    error: &str,
    store: std::sync::Arc<tokio::sync::Mutex<crate::state::JsonSnapshotStore>>,
) -> AdminNodeVersionItem {
    let report = store.lock().await.get_node_capability_report(&node_id);
    match report {
        Some(report) => AdminNodeVersionItem {
            node_id,
            xp_version: Some(report.xp_version),
            error: Some(error.to_string()),
            reported: true,
        },
        None => AdminNodeVersionItem {
            node_id,
            xp_version: None,
            error: Some(error.to_string()),
            reported: false,
        },
    }
}
