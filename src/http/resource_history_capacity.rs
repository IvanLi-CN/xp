use super::{ApiError, AppState};
use crate::state::history_repository::control::{RepositoryCapacity, RepositoryMembership};

pub(super) async fn preflight_resource_history_capacity_for_join(
    state: &AppState,
    node_id: &str,
) -> Result<(), ApiError> {
    let (node_is_new, collectible_node_count, membership) = {
        let store = state.store.lock().await;
        (
            store.get_node(node_id).is_none(),
            store.list_nodes().len() as u64 + 1,
            store.state().repository_membership.clone(),
        )
    };
    if !node_is_new {
        return Ok(());
    }
    preflight_resource_history_capacity(collectible_node_count, membership.as_ref())
}

pub(super) fn preflight_resource_history_capacity_for_membership(
    collectible_node_count: u64,
    membership: Option<&RepositoryMembership>,
) -> Result<(), ApiError> {
    preflight_resource_history_capacity(collectible_node_count, membership)
}

fn preflight_resource_history_capacity(
    collectible_node_count: u64,
    membership: Option<&RepositoryMembership>,
) -> Result<(), ApiError> {
    let Some(membership) = membership else {
        return Ok(());
    };
    let history_quota_bytes = membership
        .members()
        .iter()
        .map(|member| member.capacity().quota_bytes())
        .min()
        .unwrap_or_else(|| RepositoryCapacity::default().quota_bytes());
    let Err(error) = crate::resource_monitoring::resource_history_capacity_preflight(
        collectible_node_count,
        history_quota_bytes,
    ) else {
        return Ok(());
    };
    let mut api_error = ApiError::new(
        "resource_history_capacity_rejected",
        axum::http::StatusCode::CONFLICT,
        "resource history capacity is insufficient for the collectible cluster nodes",
    );
    api_error.details.insert(
        "required_bytes".to_string(),
        serde_json::json!(error.required_bytes),
    );
    api_error.details.insert(
        "allowed_bytes".to_string(),
        serde_json::json!(error.allowed_bytes),
    );
    Err(api_error)
}
