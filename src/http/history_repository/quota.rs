use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepositoryQuotaRequest {
    quota_bytes: u64,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    yes: bool,
}

pub(crate) async fn admin_internal_history_repository_quota(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<RepositoryQuotaRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let verified = internal
        .and_then(|Extension(auth)| auth.verified)
        .ok_or_else(|| ApiError::unauthorized("internal auth required"))?;
    if verified.context.sender_id != state.cluster.node_id {
        return Err(ApiError::unauthorized(
            "quota changes require the local node identity",
        ));
    }
    RepositoryCapacity::new(request.quota_bytes)
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    if request.apply && !request.yes {
        return Err(ApiError::invalid_request("quota apply requires yes"));
    }
    let mut runtime = state.repository_replica.lock().await;
    let previous =
        worker::repository_blocking(|| runtime.runtime_capacity()).map_err(repository_error)?;
    if request.apply {
        worker::repository_blocking(|| runtime.set_repository_quota_bytes(request.quota_bytes))
            .map_err(repository_error)?;
    }
    Ok(Json(serde_json::json!({
        "previous_quota_bytes": previous.quota_bytes(),
        "quota_bytes": request.quota_bytes,
        "used_bytes": previous.used_bytes(),
        "would_stop_history_writes": previous.used_bytes() >= request.quota_bytes,
        "applied": request.apply,
    })))
}

#[cfg(test)]
#[path = "quota_tests.rs"]
mod tests;
