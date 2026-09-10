use super::*;
use crate::control_plane_mesh::MeshPeerTarget;

#[derive(Debug, Serialize, Deserialize)]
pub(in crate::http) struct RepositoryGapSyncRequest {
    pub(super) identity: RepositoryNodeIdentity,
    pub(super) gaps: Vec<RepositoryReplicaGap>,
}

pub(super) fn source_gaps_match_identity(
    gaps: &[RepositoryReplicaGap],
    source_node_id: &str,
) -> bool {
    gaps.len() <= MAX_REPAIR_REQUEST_IDS
        && gaps.iter().all(|gap| gap.source_node_id == source_node_id)
}

pub(in crate::http) async fn admin_internal_receive_history_repository_gaps(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryGapSyncRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if !repository_identity_matches_sender(&request.identity, &verified.context.sender_id)
        || !identity_is_pinned_for_node(&state, &request.identity).await?
        || !source_gaps_match_identity(&request.gaps, request.identity.node_id().as_str())
    {
        return Err(ApiError::unauthorized("repository gap sender is invalid"));
    }
    let ready_repository_ids = ready_repository_ids(&state).await?;
    let accepts_source = state
        .repository_replica
        .lock()
        .await
        .accepts_source(
            request.identity.node_id().as_str(),
            &ready_repository_ids,
            &state.cluster.node_id,
        )
        .map_err(repository_error)?;
    if !accepts_source {
        return Err(ApiError::conflict(
            "repository is not a rendezvous collector for this source",
        ));
    }
    state
        .repository_replica
        .lock()
        .await
        .merge_replica_gaps(&request.gaps)
        .map_err(repository_error)?;
    Ok(Json(serde_json::json!({})))
}

pub(super) async fn deliver_source_gaps(
    state: &AppState,
    target: &MeshPeerTarget,
    identity: RepositoryNodeIdentity,
    gaps: Vec<RepositoryReplicaGap>,
) -> anyhow::Result<(bool, bool)> {
    if target.node_id == state.cluster.node_id {
        state
            .repository_replica
            .lock()
            .await
            .merge_replica_gaps(&gaps)?;
        return Ok((true, false));
    }
    let body = serde_json::to_vec(&RepositoryGapSyncRequest { identity, gaps })?;
    match worker::repository_direct_request::<serde_json::Value>(
        state,
        target,
        axum::http::Method::POST,
        "/api/admin/_internal/history-repository/sync-gaps",
        body,
    )
    .await
    {
        Ok(_) => Ok((true, false)),
        Err(error) => Ok((false, error.is_transport())),
    }
}

#[cfg(test)]
mod tests {
    use super::source_gaps_match_identity;
    use crate::state::history_repository::replica::RepositoryReplicaGap;

    fn gap(source_node_id: &str, permanent: bool) -> RepositoryReplicaGap {
        RepositoryReplicaGap {
            source_node_id: source_node_id.to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            first_sequence: 1,
            last_sequence: 2,
            start_unix_seconds: 1,
            end_unix_seconds: 2,
            permanent,
            reason: None,
        }
    }

    #[test]
    fn source_gap_validation_accepts_recoverable_gaps_from_the_authenticated_source() {
        assert!(source_gaps_match_identity(
            &[gap("source-a", false)],
            "source-a"
        ));
        assert!(source_gaps_match_identity(
            &[gap("source-a", true)],
            "source-a"
        ));
    }

    #[test]
    fn source_gap_validation_rejects_foreign_or_oversized_batches() {
        assert!(!source_gaps_match_identity(
            &[gap("source-b", false)],
            "source-a"
        ));
        let gaps = (0..=super::MAX_REPAIR_REQUEST_IDS)
            .map(|_| gap("source-a", false))
            .collect::<Vec<_>>();
        assert!(!source_gaps_match_identity(&gaps, "source-a"));
    }
}
