use super::*;
use super::{bootstrap, mesh_peer_target};

pub(super) fn insert_reverse_link_headers(
    headers: &mut HeaderMap,
    epoch: u64,
    rendezvous_node_id: &str,
    role: crate::reverse_mesh::ReverseRole,
    generation: u64,
) -> Result<(), ApiError> {
    for (name, value) in [
        (
            crate::reverse_mesh::REVERSE_LINK_EPOCH_HEADER,
            epoch.to_string(),
        ),
        (
            crate::reverse_mesh::REVERSE_LINK_RENDEZVOUS_HEADER,
            rendezvous_node_id.to_string(),
        ),
        (
            crate::reverse_mesh::REVERSE_LINK_ROLE_HEADER,
            role.as_str().to_string(),
        ),
        (
            crate::reverse_mesh::REVERSE_LINK_GENERATION_HEADER,
            generation.to_string(),
        ),
    ] {
        headers.insert(
            header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ApiError::invalid_request("reverse link header is invalid"))?,
            value
                .parse()
                .map_err(|_| ApiError::invalid_request("reverse link header value is invalid"))?,
        );
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
pub(in crate::http) struct ReverseLinkProbeRequest {
    target_node_id: String,
    assignment_generation: u64,
    role: crate::reverse_mesh::ReverseRole,
}

/// A target asks its assigned Rendezvous to return one signed health request through one exact
/// reverse underlay. The response is the target-side lease proof; this endpoint never forwards
/// arbitrary application traffic.
pub(in crate::http) async fn admin_internal_reverse_probe(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    Json(request): Json<ReverseLinkProbeRequest>,
) -> Result<StatusCode, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified.as_ref() else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if verified.context.route != internal_auth::InternalRoute::MeshV2
        || verified.context.sender_id != request.target_node_id
        || request.target_node_id == state.cluster.node_id
    {
        return Err(ApiError::unauthorized(
            "reverse link probe identity is invalid",
        ));
    }
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
    let assignment = {
        let store = state.store.lock().await;
        let Some(assignment) = store
            .state()
            .reverse_mesh_assignments
            .get(&request.target_node_id)
            .cloned()
        else {
            return Err(ApiError::conflict(
                "reverse relay assignment is unavailable",
            ));
        };
        if !assignment.is_valid()
            || assignment.generation != request.assignment_generation
            || assignment.credential_epoch != store.state().reverse_mesh_epoch
        {
            return Err(ApiError::conflict("reverse relay assignment is stale"));
        }
        let expected_role = bootstrap::reverse_role_for_relay(
            store.state().active_membership_operation(),
            &assignment,
            &state.cluster.node_id,
            &request.target_node_id,
            true,
        )
        .ok_or_else(|| ApiError::unauthorized("this node is not the assigned Rendezvous"))?;
        if expected_role != request.role {
            return Err(ApiError::unauthorized("reverse link probe role is invalid"));
        }
        assignment
    };
    let target = mesh_peer_target(&state, &request.target_node_id).await?;
    let rendezvous = mesh_peer_target(&state, &state.cluster.node_id).await?;
    let route = crate::control_plane_mesh::ReverseRelayRoute {
        rendezvous,
        standby_rendezvous: None,
        assignment,
        role: request.role,
    };
    state
        .mesh_client
        .send_peer_reverse_health_request_via(
            &target,
            &route,
            MeshRequest {
                method: Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_string(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(5),
                allow_ambiguous_fallback: false,
                request_id: crate::id::new_ulid_string(),
                route: internal_auth::InternalRoute::HealthV2,
                cluster_id: state.cluster.cluster_id.clone(),
                sender_id: state.cluster.node_id.clone(),
                updates_active_path: false,
            },
            ca_key_pem,
            &state.cluster_ca_pem,
        )
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(in crate::http) fn spawn_reverse_link_probe_worker(state: AppState) {
    tokio::spawn(async move {
        let links = state.reconcile.reverse_links();
        loop {
            let notified = links.probe_notified();
            let Some(link) = links.take_probe() else {
                notified.await;
                continue;
            };
            if let Err(error) = probe_reverse_link(&state, &link).await {
                tracing::debug!(
                    target_id = %link.target_node_id,
                    rendezvous_id = %link.rendezvous_node_id,
                    role = ?link.role,
                    ?error,
                    "target-side reverse link probe failed"
                );
            }
        }
    });
}

async fn probe_reverse_link(
    state: &AppState,
    link: &crate::reverse_mesh::ReverseLinkKey,
) -> Result<(), ApiError> {
    if link.target_node_id != state.cluster.node_id {
        return Err(ApiError::invalid_request(
            "reverse link target is not local",
        ));
    }
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
    let rendezvous = mesh_peer_target(state, &link.rendezvous_node_id).await?;
    let body = serde_json::to_vec(&ReverseLinkProbeRequest {
        target_node_id: link.target_node_id.clone(),
        assignment_generation: link.generation,
        role: link.role,
    })
    .map_err(|error| ApiError::internal(format!("encode reverse link probe: {error}")))?;
    state
        .mesh_client
        .send_peer_request(
            &rendezvous,
            MeshRequest {
                method: Method::POST,
                path_and_query: "/api/admin/_internal/mesh/reverse-probe".to_string(),
                content_type: Some("application/json".to_string()),
                body,
                total_budget: Duration::from_secs(5),
                allow_ambiguous_fallback: false,
                request_id: crate::id::new_ulid_string(),
                route: internal_auth::InternalRoute::MeshV2,
                cluster_id: state.cluster.cluster_id.clone(),
                sender_id: state.cluster.node_id.clone(),
                updates_active_path: false,
            },
            ca_key_pem,
            &state.cluster_ca_pem,
        )
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(())
}

pub(in crate::http) async fn admin_internal_mesh_health(
    Extension(state): Extension<AppState>,
    headers: HeaderMap,
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
        Some(internal_auth::InternalRoute::HealthV2)
    ) {
        return Err(ApiError::unauthorized(
            "mesh health authentication required",
        ));
    }
    if let Some(link) =
        crate::reverse_mesh::reverse_link_key_from_headers(&headers, &state.cluster.node_id)
            .map_err(ApiError::invalid_request)?
        && state
            .reconcile
            .reverse_links()
            .confirm_health(&link, std::time::Instant::now())
    {
        state.reconcile.request_full();
    }
    Ok(Json(json!({
        "ok": true,
        "node_id": state.cluster.node_id,
        "auth_epoch": "v2"
    })))
}
