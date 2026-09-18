use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdminMeshConfigRequest {
    pub enabled: bool,
}

pub(crate) async fn admin_update_mesh_config(
    Extension(state): Extension<AppState>,
    ApiJson(request): ApiJson<AdminMeshConfigRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _membership_operation_guard = crate::raft_membership_guard::membership_operation_gate()
        .lock_owned()
        .await;
    super::join_capability::require_mesh_gate_on_voters(&state).await?;
    super::raft_write(
        &state,
        crate::state::DesiredStateCommand::SetMeshEnabled {
            enabled: request.enabled,
        },
    )
    .await?;
    state.reconcile.request_full();
    Ok(Json(serde_json::json!({ "enabled": request.enabled })))
}
