use tokio::time::Duration;

use super::{
    AlertItem, AlertsResponse, ApiError, AppState, build_local_alerts,
    mesh::send_mesh_internal_read,
};

fn build_resource_alerts(state: &AppState) -> Vec<AlertItem> {
    state
        .resource_monitoring
        .alerts()
        .into_iter()
        .map(|alert| AlertItem {
            alert_type: alert.alert_type,
            membership_key: String::new(),
            user_id: String::new(),
            endpoint_id: String::new(),
            owner_node_id: alert.node_id.clone(),
            quota_banned: false,
            quota_banned_at: None,
            message: format!("resource threshold: {}", alert.metric),
            action_hint: "inspect node resources".to_string(),
            node_id: Some(alert.node_id.clone()),
            resource_node_id: Some(alert.node_id),
            scope: Some(alert.scope),
            metric: Some(alert.metric),
            severity: Some(alert.severity),
            opened_at: Some(alert.opened_at),
            latest_bucket_start_unix_seconds: Some(alert.latest_bucket_start_unix_seconds),
        })
        .collect()
}

pub(super) async fn admin_get_alerts_response(
    state: &AppState,
    scope: Option<&str>,
) -> Result<AlertsResponse, ApiError> {
    if let Some(scope) = scope
        && scope != "local"
    {
        return Err(ApiError::invalid_request(
            "invalid scope, expected local or omit",
        ));
    }

    let local_node_id = state.cluster.node_id.clone();
    let local_items = {
        let store = state.store.lock().await;
        let mut items = build_local_alerts(&store, &local_node_id);
        items.extend(build_resource_alerts(state));
        items
    };

    if scope == Some("local") {
        return Ok(AlertsResponse {
            partial: false,
            unreachable_nodes: Vec::new(),
            items: local_items,
        });
    }

    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let client = state.mesh_client.clone();

    let mut items = local_items;
    let mut unreachable_nodes = Vec::new();

    for node in nodes {
        if node.node_id == local_node_id {
            continue;
        }
        let base = node.api_base_url.trim_end_matches('/');
        if base.is_empty() {
            unreachable_nodes.push(node.node_id);
            continue;
        }
        let response = match send_mesh_internal_read(
            state,
            &client,
            &node,
            "/api/admin/_internal/alerts".to_string(),
            Duration::from_secs(3),
        )
        .await
        {
            Ok(response) => response,
            _ => {
                unreachable_nodes.push(node.node_id);
                continue;
            }
        };

        if !response.status().is_success() {
            unreachable_nodes.push(node.node_id);
            continue;
        }

        match response.json::<AlertsResponse>().await {
            Ok(remote) => items.extend(remote.items),
            Err(_) => unreachable_nodes.push(node.node_id),
        }
    }

    let partial = !unreachable_nodes.is_empty();
    Ok(AlertsResponse {
        partial,
        unreachable_nodes,
        items,
    })
}
