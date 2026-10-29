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

    let items = stream::iter(nodes.into_iter().map(|node| {
        let client = client.clone();
        let cluster_id = cluster_id.clone();
        let local_node_id = local_node_id.clone();
        async move {
            if node.node_id == local_node_id {
                return AdminNodeVersionItem {
                    node_id: node.node_id,
                    xp_version: Some(crate::version::VERSION.to_string()),
                    error: None,
                };
            }

            let remaining = fanout_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return AdminNodeVersionItem {
                    node_id: node.node_id,
                    xp_version: None,
                    error: Some("timeout".to_string()),
                };
            }

            let (response, deadline) = match client
                .send_registered_public_read(&node.api_base_url, "/api/cluster/info", remaining)
                .await
            {
                Ok(response) => response,
                Err(_) => {
                    return AdminNodeVersionItem {
                        node_id: node.node_id,
                        xp_version: None,
                        error: Some("unreachable".to_string()),
                    };
                }
            };

            if !response.status().is_success() {
                return AdminNodeVersionItem {
                    node_id: node.node_id,
                    xp_version: None,
                    error: Some(format!("http_{}", response.status().as_u16())),
                };
            }

            let body = match bounded_json::read_bounded_internal_json::<ClusterInfoVersionResponse>(
                response,
                deadline.saturating_duration_since(std::time::Instant::now()),
            )
            .await
            {
                Ok(body) => body,
                Err(_) => {
                    return AdminNodeVersionItem {
                        node_id: node.node_id,
                        xp_version: None,
                        error: Some("invalid_response".to_string()),
                    };
                }
            };

            if body.cluster_id != cluster_id || body.node_id != node.node_id {
                return AdminNodeVersionItem {
                    node_id: node.node_id,
                    xp_version: None,
                    error: Some("cluster_mismatch".to_string()),
                };
            }

            AdminNodeVersionItem {
                node_id: node.node_id,
                xp_version: Some(body.xp_version),
                error: None,
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
