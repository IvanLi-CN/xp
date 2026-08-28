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

fn reverse_link_health_proof_is_valid(
    headers: &HeaderMap,
    link: &crate::reverse_mesh::ReverseLinkKey,
    local_node_id: &str,
    cluster_ca_key_pem: &str,
    context: &internal_auth::RequestContext,
) -> bool {
    let Some(inner_signature) = headers
        .get(internal_auth::INTERNAL_SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Ok(envelope) = crate::reverse_mesh::ReverseRelayEnvelope::from_headers(headers) else {
        return false;
    };
    let expected_authority =
        crate::reverse_mesh::derive_reverse_origin(&crate::reverse_mesh::derive_reverse_origin_id(
            link.epoch,
            local_node_id,
            &link.rendezvous_node_id,
            link.role,
            link.generation,
        ));
    envelope.verify(cluster_ca_key_pem)
        && envelope.assignment_generation == link.generation
        && envelope.target_node_id == local_node_id
        && envelope.method == Method::GET.as_str()
        && envelope.uri == "/api/admin/_internal/mesh/health"
        && envelope.content_type.is_empty()
        && envelope.route == internal_auth::InternalRoute::HealthV2.as_str()
        && envelope.sender_node_id == link.rendezvous_node_id
        && envelope.sender_node_id == context.sender_id
        && envelope.request_id == context.request_id
        && envelope.issued_at == context.issued_at
        && envelope.content_length == 0
        && envelope.reverse_authority == expected_authority
        && envelope.inner_signature == inner_signature
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
    let link = crate::reverse_mesh::reverse_link_key_from_headers(&headers, &state.cluster.node_id)
        .map_err(ApiError::invalid_request)?;
    if link.is_none() && headers.contains_key(crate::reverse_mesh::RELAY_VERSION_HEADER) {
        return Err(ApiError::unauthorized(
            "reverse relay proof requires complete reverse link headers",
        ));
    }
    if let Some(link) = link {
        let verified = internal
            .verified
            .as_ref()
            .expect("verified internal health request");
        let ca_key_pem = state
            .cluster_ca_key_pem
            .as_deref()
            .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
        if !reverse_link_health_proof_is_valid(
            &headers,
            &link,
            &state.cluster.node_id,
            ca_key_pem,
            &verified.context,
        ) {
            return Err(ApiError::unauthorized(
                "reverse link health proof is invalid",
            ));
        }
        if state
            .reconcile
            .reverse_links()
            .confirm_health(&link, std::time::Instant::now())
        {
            state.reconcile.request_full();
        }
    }
    Ok(Json(json!({
        "ok": true,
        "node_id": state.cluster.node_id,
        "auth_epoch": "v2"
    })))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn proof_headers(sender: &str) -> (HeaderMap, crate::reverse_mesh::ReverseLinkKey) {
        let link = crate::reverse_mesh::ReverseLinkKey::new(
            7,
            "target",
            "rendezvous",
            crate::reverse_mesh::ReverseRole::Primary,
            3,
        );
        let mut headers = HeaderMap::new();
        insert_reverse_link_headers(
            &mut headers,
            link.epoch,
            &link.rendezvous_node_id,
            link.role,
            link.generation,
        )
        .unwrap();
        headers.insert(
            internal_auth::INTERNAL_SIGNATURE_HEADER,
            HeaderValue::from_static("v2:inner"),
        );
        let authority = crate::reverse_mesh::derive_reverse_origin(
            &crate::reverse_mesh::derive_reverse_origin_id(
                link.epoch,
                "target",
                &link.rendezvous_node_id,
                link.role,
                link.generation,
            ),
        );
        let mut envelope = crate::reverse_mesh::ReverseRelayEnvelope {
            version: String::new(),
            assignment_generation: link.generation,
            target_node_id: "target".to_string(),
            method: "GET".to_string(),
            uri: "/api/admin/_internal/mesh/health".to_string(),
            content_type: String::new(),
            route: internal_auth::InternalRoute::HealthV2.as_str().to_string(),
            sender_node_id: sender.to_string(),
            request_id: "request".to_string(),
            issued_at: 1,
            content_length: 0,
            reverse_authority: authority,
            inner_signature: "v2:inner".to_string(),
            outer_signature: String::new(),
        };
        envelope.sign("cluster-key");
        envelope.insert_headers(&mut headers).unwrap();
        (headers, link)
    }

    fn health_context(sender_id: &str) -> internal_auth::RequestContext {
        internal_auth::RequestContext {
            route: internal_auth::InternalRoute::HealthV2,
            cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
            sender_id: sender_id.to_string(),
            target_id: "target".to_string(),
            request_id: "request".to_string(),
            issued_at: 1,
        }
    }

    #[test]
    fn reverse_link_lease_requires_the_assigned_rendezvous_relay_proof() {
        let (headers, link) = proof_headers("rendezvous");
        assert!(reverse_link_health_proof_is_valid(
            &headers,
            &link,
            "target",
            "cluster-key",
            &health_context("rendezvous"),
        ));

        let (foreign_headers, link) = proof_headers("other-member");
        assert!(!reverse_link_health_proof_is_valid(
            &foreign_headers,
            &link,
            "target",
            "cluster-key",
            &health_context("other-member"),
        ));
    }

    #[test]
    fn reverse_link_headers_on_direct_health_cannot_extend_a_lease() {
        let (mut headers, link) = proof_headers("rendezvous");
        for name in [
            crate::reverse_mesh::RELAY_VERSION_HEADER,
            crate::reverse_mesh::RELAY_GENERATION_HEADER,
            crate::reverse_mesh::RELAY_TARGET_HEADER,
            crate::reverse_mesh::RELAY_METHOD_HEADER,
            crate::reverse_mesh::RELAY_URI_HEADER,
            crate::reverse_mesh::RELAY_CONTENT_TYPE_HEADER,
            crate::reverse_mesh::RELAY_ROUTE_HEADER,
            crate::reverse_mesh::RELAY_SENDER_HEADER,
            crate::reverse_mesh::RELAY_REQUEST_ID_HEADER,
            crate::reverse_mesh::RELAY_ISSUED_AT_HEADER,
            crate::reverse_mesh::RELAY_CONTENT_LENGTH_HEADER,
            crate::reverse_mesh::RELAY_REVERSE_AUTHORITY_HEADER,
            crate::reverse_mesh::RELAY_INNER_SIGNATURE_HEADER,
            crate::reverse_mesh::RELAY_OUTER_SIGNATURE_HEADER,
        ] {
            headers.remove(name);
        }
        assert!(!reverse_link_health_proof_is_valid(
            &headers,
            &link,
            "target",
            "cluster-key",
            &health_context("rendezvous"),
        ));
    }
}
