use super::*;
use crate::control_plane_mesh::peer_target_from_node;
use axum::http::Method;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginalSourceRequest {
    collector_node_id: String,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    yes: bool,
    expected_fingerprint: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginalProofRequest {
    segment_ids: Vec<String>,
}

pub(crate) async fn original_proof(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<OriginalProofRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_cluster_sender(&state, internal).await?;
    if request.segment_ids.is_empty()
        || request.segment_ids.len() > 32
        || request
            .segment_ids
            .iter()
            .any(|id| id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(ApiError::invalid_request(
            "invalid original segment proof page",
        ));
    }
    let runtime = state.repository_replica.lock().await;
    let ids = worker::repository_blocking(|| {
        runtime.confirmed_original_segment_ids(&request.segment_ids)
    })
    .map_err(repository_error)?;
    Ok(Json(serde_json::json!({"segment_ids": ids})))
}

pub(crate) async fn original_source(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<OriginalSourceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let verified = internal
        .and_then(|Extension(auth)| auth.verified)
        .ok_or_else(|| ApiError::unauthorized("internal auth required"))?;
    if verified.context.sender_id != state.cluster.node_id {
        return Err(ApiError::unauthorized(
            "original source recovery requires local identity",
        ));
    }
    if request.apply && (!request.yes || request.expected_fingerprint.is_none()) {
        return Err(ApiError::invalid_request(
            "original source apply requires yes and fingerprint",
        ));
    }
    let binding = recovery_preflight::verify_cluster(&state).await?;
    let page = {
        let runtime = state.repository_replica.lock().await;
        worker::repository_blocking(|| runtime.original_source_page()).map_err(repository_error)?
    };
    if page.is_empty() {
        return Ok(Json(
            serde_json::json!({"complete": true, "applied": false, "requests": []}),
        ));
    }
    let peer = {
        let store = state.store.lock().await;
        for segment in &page {
            if store
                .state()
                .nodes
                .contains_key(segment.identity.node_id().as_str())
            {
                return Err(ApiError::conflict(
                    "current source pages must use ordinary delivery",
                ));
            }
        }
        let ready = store
            .state()
            .repository_membership
            .as_ref()
            .is_some_and(|membership| {
                membership.members().iter().any(|member| {
                    member.identity().node_id().as_str() == request.collector_node_id
                        && member.lifecycle() == &RepositoryLifecycle::Ready
                })
            });
        if !ready {
            return Err(ApiError::conflict(
                "collector must be a configured Ready repository",
            ));
        }
        let node = store
            .state()
            .nodes
            .get(&request.collector_node_id)
            .ok_or_else(|| ApiError::conflict("collector node is absent"))?;
        let peer = peer_target_from_node(node, &store.list_endpoints());
        if !peer.public_base_url.starts_with("https://") {
            return Err(ApiError::conflict(
                "collector requires registered public HTTPS",
            ));
        }
        peer
    };
    let mut requests = Vec::with_capacity(page.len());
    let mut ids = Vec::with_capacity(page.len());
    for segment in &page {
        requests.push(
            RepositorySyncRequest::with_wire(segment.identity.clone(), &segment.wire, Vec::new())
                .map_err(|error| ApiError::internal(error.to_string()))?,
        );
        ids.push(hex::encode(Sha256::digest(&segment.wire)));
    }
    let raw_fingerprint = hex::encode(Sha256::digest(
        serde_json::to_vec(&(&request.collector_node_id, &requests))
            .map_err(|error| ApiError::internal(error.to_string()))?,
    ));
    let fingerprint = recovery_preflight::bind_fingerprint(&raw_fingerprint, &binding)?;
    if !request.apply {
        return Ok(Json(serde_json::json!({
            "fingerprint": fingerprint,
            "complete": false,
            "applied": false,
            "requests": requests,
        })));
    }
    if request.expected_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(ApiError::conflict(
            "original source recovery fingerprint changed",
        ));
    }
    let body = serde_json::to_vec(&serde_json::json!({"segment_ids":ids}))
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let response = worker::repository_direct_request_body(
        &state,
        &peer,
        Method::POST,
        "/api/admin/_internal/history-repository/original-proof",
        body,
        16 * 1024,
    )
    .await
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    let proof: serde_json::Value =
        serde_json::from_slice(&response).map_err(|error| ApiError::conflict(error.to_string()))?;
    ids.sort();
    if proof["segment_ids"] != serde_json::json!(ids) {
        return Err(ApiError::conflict(
            "collector has not confirmed every original wire",
        ));
    }
    let mut runtime = state.repository_replica.lock().await;
    let store = state
        .store
        .try_lock()
        .map_err(|_| ApiError::conflict("source cluster state is busy"))?;
    if recovery_preflight::binding_for_repository(&state, &store.state().repository_membership)?
        != binding
    {
        return Err(ApiError::conflict("source recovery cluster view changed"));
    }
    let current =
        worker::repository_blocking(|| runtime.original_source_page()).map_err(repository_error)?;
    if current.len() != page.len()
        || current
            .iter()
            .zip(&page)
            .any(|(a, b)| a.identity != b.identity || a.wire != b.wire)
    {
        return Err(ApiError::conflict("source recovery pending page changed"));
    }
    worker::repository_blocking(|| {
        runtime.acknowledge_original_source_page(
            &page,
            u64::try_from(Utc::now().timestamp()).unwrap_or_default(),
            "direct",
        )
    })
    .map_err(repository_error)?;
    Ok(Json(
        serde_json::json!({"applied":true,"acknowledged_segment_ids":ids}),
    ))
}

pub(crate) fn routes() -> axum::Router {
    use axum::routing::post;
    axum::Router::new()
        .route(
            "/_internal/history-repository/original-source",
            post(original_source),
        )
        .route(
            "/_internal/history-repository/original-proof",
            post(original_proof),
        )
        .route(
            "/_internal/history-repository/original-repair",
            post(super::original_repair::admin_internal_original_repair),
        )
}
