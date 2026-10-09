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
    cluster_preflight: super::recovery_preflight::RecoveryClusterBinding,
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
    let cluster_preflight = super::recovery_preflight::verify_cluster(&state).await?;
    let mut runtime = state.repository_replica.lock().await;
    // Never await the store while holding the replica lock. A busy or changed store fails closed.
    let store = state
        .store
        .try_lock()
        .map_err(|_| ApiError::conflict("history recovery cluster state is busy"))?;
    if super::recovery_preflight::binding_for_repository(
        &state,
        &store.state().repository_membership,
    )? != cluster_preflight
    {
        return Err(ApiError::conflict("history recovery cluster view changed"));
    }
    let peer = crate::state::history_repository::identity::RepositoryNodeId::try_from(
        request.peer_node_id.clone(),
    )
    .map_err(|_| ApiError::invalid_request("peer node id is invalid"))?;
    if !store
        .state()
        .repository_membership
        .as_ref()
        .and_then(|membership| membership.repository(&peer))
        .is_some_and(|member| member.lifecycle() == &RepositoryLifecycle::Ready)
    {
        return Err(ApiError::conflict(
            "history recovery requires a Ready repository peer",
        ));
    }
    let mut preview = runtime
        .preview_initial_peer_recovery(&request.peer_node_id)
        .map_err(repository_error)?;
    let runtime_fingerprint = preview.fingerprint.clone();
    preview.fingerprint =
        super::recovery_preflight::bind_fingerprint(&runtime_fingerprint, &cluster_preflight)?;
    if !request.apply {
        return Ok(Json(RepositoryRecoveryResponse {
            preview,
            applied: false,
            cluster_preflight,
        }));
    }
    let expected = request
        .expected_recovery_fingerprint
        .as_deref()
        .ok_or_else(|| ApiError::invalid_request("history recovery apply requires fingerprint"))?;
    if expected != preview.fingerprint {
        return Err(ApiError::conflict("history recovery fingerprint changed"));
    }
    let mut applied_preview = runtime
        .arm_initial_peer_recovery(&request.peer_node_id, &runtime_fingerprint)
        .map_err(repository_error)?;
    applied_preview.fingerprint = preview.fingerprint;
    Ok(Json(RepositoryRecoveryResponse {
        preview: applied_preview,
        applied: true,
        cluster_preflight,
    }))
}
