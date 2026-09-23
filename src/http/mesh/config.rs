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
    if request.enabled
        && let Err(failures) = super::run_mesh_enable_preflight(state.clone()).await
    {
        let mut error = ApiError::new(
            "mesh_preflight_failed",
            StatusCode::CONFLICT,
            "all current voter directions must pass Direct Mesh preflight",
        );
        error.details.insert(
            "failures".to_string(),
            serde_json::to_value(failures)
                .map_err(|encode| ApiError::internal(encode.to_string()))?,
        );
        return Err(error);
    }
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
