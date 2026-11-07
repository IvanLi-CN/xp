use super::*;
use crate::control_plane_mesh::peer_target_from_node;
use crate::state::history_repository::replica::{PrefixRecoveryBundle, PrefixRecoveryPlan};
use axum::http::Method;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceRequest {
    collector_node_id: String,
    plans: Vec<PrefixRecoveryPlan>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportRequest {
    bundle: PrefixRecoveryBundle,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    yes: bool,
    expected_fingerprint: Option<String>,
}

fn require_local(
    state: &AppState,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<(), ApiError> {
    let verified = internal
        .and_then(|Extension(auth)| auth.verified)
        .ok_or_else(|| ApiError::unauthorized("internal auth required"))?;
    if verified.context.sender_id != state.cluster.node_id {
        return Err(ApiError::unauthorized(
            "prefix recovery requires local node identity",
        ));
    }
    Ok(())
}

async fn binding(state: &AppState) -> Result<String, ApiError> {
    let cluster = recovery_preflight::verify_cluster(state).await?;
    let store = state.store.lock().await;
    if recovery_preflight::binding_for_repository(state, &store.state().repository_membership)?
        != cluster
    {
        return Err(ApiError::conflict("prefix recovery cluster view changed"));
    }
    binding_locked(state, &store, &cluster)
}

fn binding_locked(
    state: &AppState,
    store: &crate::state::JsonSnapshotStore,
    cluster: &recovery_preflight::RecoveryClusterBinding,
) -> Result<String, ApiError> {
    let peers = store
        .state()
        .nodes
        .iter()
        .map(|(id, node)| (id, &node.api_base_url))
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(state.cluster.cluster_id.as_str(), cluster, peers))
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn require_ready(
    state: &AppState,
    store: &crate::state::JsonSnapshotStore,
    id: &str,
) -> Result<(), ApiError> {
    if !store
        .state()
        .repository_membership
        .as_ref()
        .is_some_and(|membership| {
            membership.members().iter().any(|member| {
                member.identity().node_id().as_str() == id
                    && member.lifecycle() == &RepositoryLifecycle::Ready
            })
        })
    {
        return Err(ApiError::conflict(
            "prefix recovery collector must be Ready",
        ));
    }
    if !store.state().nodes.contains_key(&state.cluster.node_id) {
        return Err(ApiError::conflict("prefix recovery local node is absent"));
    }
    Ok(())
}

pub(crate) async fn source(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<SourceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // A collector can re-attest only its own bundle; local xp-ops can export a preview.
    let sender = internal
        .and_then(|Extension(auth)| auth.verified)
        .ok_or_else(|| ApiError::unauthorized("internal auth required"))?
        .context
        .sender_id;
    if sender != state.cluster.node_id && sender != request.collector_node_id {
        return Err(ApiError::unauthorized(
            "prefix source sender is not its collector",
        ));
    }
    let bound = binding(&state).await?;
    let (deletion_version, deletions) = state
        .node_history
        .repository_deletion_snapshot(&request.collector_node_id)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let identity = derived_repository_identity(
        &state,
        RepositoryNodeId::try_from(state.cluster.node_id.clone())
            .map_err(|error| ApiError::internal(error.to_string()))?,
    )?;
    let key = derived_repository_signing_key(&state, &state.cluster.node_id)?;
    let runtime = state.repository_replica.lock().await;
    let store = state
        .store
        .try_lock()
        .map_err(|_| ApiError::conflict("prefix source cluster state is busy"))?;
    let current =
        recovery_preflight::binding_for_repository(&state, &store.state().repository_membership)?;
    if binding_locked(&state, &store, &current)? != bound {
        return Err(ApiError::conflict("prefix source cluster binding changed"));
    }
    require_ready(&state, &store, &request.collector_node_id)?;
    let bundle = worker::repository_blocking(|| {
        runtime.export_prefix_recovery(
            &state.cluster.cluster_id,
            &request.collector_node_id,
            identity,
            &key,
            bound,
            request.plans,
            deletion_version,
            &deletions,
        )
    })
    .map_err(repository_error)?;
    drop(store);
    drop(runtime);
    let (current_version, _) = state
        .node_history
        .repository_deletion_snapshot(&request.collector_node_id)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    if current_version != deletion_version {
        return Err(ApiError::conflict("prefix source deletion version changed"));
    }
    Ok(Json(serde_json::json!({"bundle": bundle})))
}

pub(crate) async fn import(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<ImportRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_local(&state, internal)?;
    if request.apply && (!request.yes || request.expected_fingerprint.is_none()) {
        return Err(ApiError::invalid_request(
            "prefix apply requires yes and fingerprint",
        ));
    }
    let bundle = request.bundle;
    bundle.verify().map_err(repository_error)?;
    if bundle.cluster_id != state.cluster.cluster_id
        || bundle.collector_node_id != state.cluster.node_id
    {
        return Err(ApiError::conflict(
            "prefix recovery cluster or collector mismatch",
        ));
    }
    let bound = binding(&state).await?;
    if bundle.cluster_binding != bound {
        return Err(ApiError::conflict(
            "prefix recovery cluster binding changed",
        ));
    }
    let peer = {
        let store = state.store.lock().await;
        require_ready(&state, &store, &state.cluster.node_id)?;
        let node = store
            .state()
            .nodes
            .get(bundle.identity.node_id().as_str())
            .ok_or_else(|| ApiError::conflict("prefix recovery source is absent"))?;
        let expected = derived_repository_identity(&state, bundle.identity.node_id().clone())?;
        if expected != bundle.identity {
            return Err(ApiError::unauthorized("prefix source identity mismatch"));
        }
        let peer = peer_target_from_node(node, &store.list_endpoints());
        if !peer.public_base_url.starts_with("https://") {
            return Err(ApiError::conflict(
                "prefix source requires registered public HTTPS",
            ));
        }
        peer
    };
    // Re-read the current source inventory and deletion state before every preview or apply.
    // A saved bundle alone never grants recovery against a changed durable outbox.
    let body = serde_json::to_vec(&SourceRequest {
        collector_node_id: state.cluster.node_id.clone(),
        plans: bundle
            .streams
            .iter()
            .map(|stream| stream.plan.clone())
            .collect(),
    })
    .map_err(|error| ApiError::internal(error.to_string()))?;
    let fresh = worker::repository_direct_request_body(
        &state,
        &peer,
        Method::POST,
        "/api/admin/_internal/history-repository/prefix-source",
        body,
        1024 * 1024,
    )
    .await
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    #[derive(Deserialize)]
    struct SourceResponse {
        bundle: PrefixRecoveryBundle,
    }
    let fresh: SourceResponse =
        serde_json::from_slice(&fresh).map_err(|error| ApiError::conflict(error.to_string()))?;
    fresh.bundle.verify().map_err(repository_error)?;
    if fresh.bundle.digest().map_err(repository_error)?
        != bundle.digest().map_err(repository_error)?
    {
        return Err(ApiError::conflict(
            "prefix source inventory or pending head changed",
        ));
    }
    let mut runtime = state.repository_replica.lock().await;
    let store = state
        .store
        .try_lock()
        .map_err(|_| ApiError::conflict("prefix collector cluster state is busy"))?;
    let current =
        recovery_preflight::binding_for_repository(&state, &store.state().repository_membership)?;
    if binding_locked(&state, &store, &current)? != bound {
        return Err(ApiError::conflict(
            "prefix collector cluster binding changed",
        ));
    }
    require_ready(&state, &store, &state.cluster.node_id)?;
    let fingerprint = runtime
        .prefix_recovery_fingerprint(&bundle)
        .map_err(repository_error)?;
    if !request.apply {
        return Ok(Json(
            serde_json::json!({"fingerprint":fingerprint,"applied":false,
            "reason":"operator_confirmed_unrecoverable_prefix","bundle":bundle}),
        ));
    }
    if request.expected_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(ApiError::conflict("prefix recovery fingerprint changed"));
    }
    let ready = store
        .state()
        .repository_membership
        .as_ref()
        .expect("Ready checked")
        .ready_members()
        .map(|member| member.node_id().as_str().to_owned())
        .collect::<Vec<_>>();
    let receipts = worker::repository_blocking(|| {
        runtime.apply_prefix_recovery(
            &bundle,
            &fingerprint,
            u64::try_from(Utc::now().timestamp()).unwrap_or_default(),
            &ready,
            &state.cluster.node_id,
        )
    })
    .map_err(repository_error)?;
    Ok(Json(
        serde_json::json!({"applied":true,"receipts":receipts,"historical_loss":true}),
    ))
}

pub(crate) async fn evidence(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_local(&state, internal)?;
    let runtime = state.repository_replica.lock().await;
    Ok(Json(
        serde_json::json!({"recoveries":runtime.prefix_recovery_evidence()}),
    ))
}

pub(crate) fn routes() -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/_internal/history-repository/prefix-source", post(source))
        .route("/_internal/history-repository/prefix-import", post(import))
        .route(
            "/_internal/history-repository/prefix-evidence",
            get(evidence),
        )
}

#[cfg(test)]
#[path = "prefix_recovery_tests.rs"]
mod tests;
