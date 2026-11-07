use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginalRepairRequest {
    requests: Vec<RepositorySyncRequest>,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    yes: bool,
    expected_fingerprint: Option<String>,
}

pub(crate) async fn admin_internal_original_repair(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<OriginalRepairRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let verified = internal
        .and_then(|Extension(auth)| auth.verified)
        .ok_or_else(|| ApiError::unauthorized("internal auth required"))?;
    if verified.context.sender_id != state.cluster.node_id {
        return Err(ApiError::unauthorized(
            "original history import requires the local node identity",
        ));
    }
    if request.requests.is_empty()
        || request.requests.len() > batch::MAX_HISTORY_SYNC_BATCH_SEGMENTS
    {
        return Err(ApiError::invalid_request(
            "original history import page exceeds segment bound",
        ));
    }
    if request.apply && (!request.yes || request.expected_fingerprint.is_none()) {
        return Err(ApiError::invalid_request(
            "original history import apply requires yes and fingerprint",
        ));
    }
    let identity = request.requests[0].identity.clone();
    let mut total = 0_usize;
    let now = u64::try_from(Utc::now().timestamp()).unwrap_or_default();
    let mut wires = Vec::with_capacity(request.requests.len());
    for item in request.requests {
        if item.identity != identity || !item.gaps.is_empty() {
            return Err(ApiError::invalid_request(
                "original history import requires one identity and no gaps",
            ));
        }
        let wire = item.decode_wire()?;
        total = total
            .checked_add(wire.len())
            .ok_or_else(|| ApiError::invalid_request("original history import byte overflow"))?;
        if total > batch::MAX_HISTORY_SYNC_BATCH_WIRE_BYTES {
            return Err(ApiError::invalid_request(
                "original history import exceeds byte bound",
            ));
        }
        wires.push((wire, now));
    }
    if !identity_is_valid_for_history_replay(&state, &identity).await? {
        return Err(ApiError::unauthorized(
            "original history identity is invalid",
        ));
    }
    let binding = recovery_preflight::verify_cluster(&state).await?;
    let mut runtime = state.repository_replica.lock().await;
    let store = state
        .store
        .try_lock()
        .map_err(|_| ApiError::conflict("original history import cluster state is busy"))?;
    if recovery_preflight::binding_for_repository(&state, &store.state().repository_membership)?
        != binding
    {
        return Err(ApiError::conflict(
            "original history import cluster view changed",
        ));
    }
    if store
        .state()
        .nodes
        .contains_key(identity.node_id().as_str())
    {
        return Err(ApiError::conflict(
            "current sources must use ordinary history delivery",
        ));
    }
    let ready = store
        .state()
        .repository_membership
        .as_ref()
        .map(|membership| {
            membership
                .members()
                .iter()
                .filter(|member| member.lifecycle() == &RepositoryLifecycle::Ready)
                .map(|member| member.identity().node_id().as_str().to_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !ready.contains(&state.cluster.node_id) {
        return Err(ApiError::conflict(
            "original history import requires a Ready local repository",
        ));
    }
    let fingerprint = recovery_preflight::bind_fingerprint(
        &runtime
            .original_repair_fingerprint(&state.cluster.cluster_id, &identity, &wires)
            .map_err(repository_error)?,
        &binding,
    )?;
    if !request.apply {
        return Ok(Json(
            serde_json::json!({"fingerprint": fingerprint, "applied": false,
            "segments": wires.len(), "canonical_bytes": total}),
        ));
    }
    if request.expected_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(ApiError::conflict(
            "original history import fingerprint changed",
        ));
    }
    runtime
        .protect_original_repair_handoff(&identity, now)
        .map_err(repository_error)?;
    let receipts = worker::repository_blocking(|| {
        runtime.receive_repair_wires_with_receipts(
            &state.cluster.cluster_id,
            &identity,
            &wires,
            &ready,
            &state.cluster.node_id,
        )
    })
    .map_err(repository_error)?;
    Ok(Json(
        serde_json::json!({"fingerprint": fingerprint, "applied": true, "receipts": receipts}),
    ))
}

#[cfg(test)]
#[path = "original_repair_tests.rs"]
mod tests;
