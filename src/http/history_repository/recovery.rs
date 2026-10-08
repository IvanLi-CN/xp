use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepositoryRecoveryRequest {
    peer_node_id: String,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    yes: bool,
    #[serde(default)]
    expected_recovery_fingerprint: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RepositoryRecoveryResponse {
    preview: InitialPeerRecoveryPreview,
    applied: bool,
}

pub(crate) async fn admin_internal_history_repository_recovery(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryRecoveryRequest>,
) -> Result<Json<RepositoryRecoveryResponse>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if verified.context.sender_id != state.cluster.node_id {
        return Err(ApiError::unauthorized(
            "history recovery must be signed by the local node",
        ));
    }
    {
        let store = state.store.lock().await;
        let Some(membership) = store.state().repository_membership.as_ref() else {
            return Err(ApiError::conflict(
                "history repository membership is not configured",
            ));
        };
        let peer = crate::state::history_repository::identity::RepositoryNodeId::try_from(
            request.peer_node_id.clone(),
        )
        .map_err(|_| ApiError::invalid_request("peer node id is invalid"))?;
        if !membership
            .repository(&peer)
            .is_some_and(|member| member.lifecycle() == &RepositoryLifecycle::Ready)
        {
            return Err(ApiError::conflict(
                "history recovery requires a Ready repository peer",
            ));
        }
    }
    if request.apply && !request.yes {
        return Err(ApiError::invalid_request(
            "history recovery apply requires yes",
        ));
    }
    let mut runtime = state.repository_replica.lock().await;
    let preview = runtime
        .preview_initial_peer_recovery(&request.peer_node_id)
        .map_err(repository_error)?;
    if !request.apply {
        return Ok(Json(RepositoryRecoveryResponse {
            preview,
            applied: false,
        }));
    }
    let expected = request
        .expected_recovery_fingerprint
        .as_deref()
        .ok_or_else(|| ApiError::invalid_request("history recovery apply requires fingerprint"))?;
    let applied_preview = runtime
        .arm_initial_peer_recovery(&request.peer_node_id, expected)
        .map_err(repository_error)?;
    Ok(Json(RepositoryRecoveryResponse {
        preview: applied_preview,
        applied: true,
    }))
}
