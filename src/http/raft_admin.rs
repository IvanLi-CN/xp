use super::*;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ClusterInfoResponse {
    pub(super) cluster_id: String,
    pub(super) node_id: String,
    pub(super) role: &'static str,
    pub(super) leader_api_base_url: String,
    pub(super) term: u64,
    pub(super) xp_version: String,
}

pub(crate) fn build_cluster_info_response(state: &AppState) -> ClusterInfoResponse {
    let metrics = raft_metrics(state);
    let leader_api_base_url = leader_api_base_url(&metrics).unwrap_or_default();
    let role = if is_leader(&metrics) {
        "leader"
    } else {
        "follower"
    };
    ClusterInfoResponse {
        cluster_id: state.cluster.cluster_id.clone(),
        node_id: state.cluster.node_id.clone(),
        role,
        leader_api_base_url,
        term: metrics.current_term,
        xp_version: crate::version::VERSION.to_string(),
    }
}

pub(crate) async fn cluster_info(
    Extension(state): Extension<AppState>,
) -> Result<Json<ClusterInfoResponse>, ApiError> {
    Ok(Json(build_cluster_info_response(&state)))
}

pub(crate) async fn admin_internal_raft_elect(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if !matches!(
        internal
            .verified
            .as_ref()
            .map(|request| request.context.route),
        Some(internal_auth::InternalRoute::MeshV2)
    ) {
        return Err(ApiError::unauthorized("mesh internal auth required"));
    }

    let metrics = raft_metrics(&state);
    let local_id = metrics.id;
    if metrics
        .membership_config
        .membership()
        .get_joint_config()
        .len()
        > 1
    {
        return Err(ApiError::conflict(
            "Raft membership is in joint consensus; refusing leader election",
        ));
    }
    if !metrics
        .membership_config
        .membership()
        .voter_ids()
        .any(|voter_id| voter_id == local_id)
    {
        return Err(ApiError::conflict(
            "local node is not a Raft voter; refusing leader election",
        ));
    }
    if state
        .store
        .lock()
        .await
        .state()
        .active_membership_operation()
        .is_some()
    {
        return Err(ApiError::conflict(
            "a membership operation is active; refusing leader election",
        ));
    }
    if let Some(last_log_index) = metrics.last_log_index
        && metrics
            .last_applied
            .is_none_or(|last_applied| last_log_index > last_applied.index)
    {
        return Err(ApiError::conflict(
            "local Raft log is not applied; refusing leader election",
        ));
    }

    state.raft.elect().await.map_err(|error| {
        ApiError::conflict(format!("leader election did not complete: {error}"))
    })?;

    let info = build_cluster_info_response(&state);
    Ok(Json(serde_json::json!({
        "node_id": info.node_id,
        "role": info.role,
        "leader_api_base_url": info.leader_api_base_url,
        "term": info.term,
        "xp_version": info.xp_version,
    })))
}
