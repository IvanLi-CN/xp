use super::*;
use crate::control_plane_mesh::MeshRequestError;
use crate::http::endpoint_requests::EndpointMutationForwardRequest;
use crate::internal_auth::InternalRoute;
use std::time::Duration;

pub(super) async fn admin_create_endpoint(
    Extension(state): Extension<AppState>,
    headers: HeaderMap,
    ApiJson(req): ApiJson<CreateEndpointRequest>,
) -> Result<Json<Endpoint>, ApiError> {
    let request_id = request_id_from_headers(&headers);
    if create_contains_xmux(&req) && !super::is_leader(&super::raft_metrics(&state)) {
        return forward_endpoint_mutation(
            &state,
            EndpointMutationForwardRequest::Create { request: req },
            request_id,
        )
        .await;
    }
    admin_create_endpoint_local(state, req).await
}

async fn admin_create_endpoint_local(
    state: AppState,
    req: CreateEndpointRequest,
) -> Result<Json<Endpoint>, ApiError> {
    let _xmux_guard = if matches!(
        &req,
        CreateEndpointRequest::VlessRealityVisionTcp {
            mihomo_xmux: Some(_),
            ..
        }
    ) {
        join_capability::lock_xmux_settings_change(&state).await?
    } else {
        None
    };
    let endpoint = match req {
        CreateEndpointRequest::VlessRealityVisionTcp {
            node_id,
            port,
            reality,
            canary_upstream,
            accepted_authorities,
            mihomo_smux,
            mihomo_xmux,
            transport,
        } => {
            let mihomo_smux = mihomo_smux.unwrap_or_default();
            mihomo_smux.validate().map_err(ApiError::invalid_request)?;
            if let Some(xmux) = &mihomo_xmux {
                xmux.validate().map_err(ApiError::invalid_request)?;
            }
            let mihomo_xmux = mihomo_xmux.filter(|value| *value != Default::default());
            let transport = transport.unwrap_or(crate::protocol::VlessRealityTransport::Xhttp);
            let store = state.store.lock().await;
            if let Some(reality) = reality {
                if canary_upstream.is_some() {
                    return Err(ApiError::invalid_request(
                        "canary_upstream is only editable on managed VLESS endpoints",
                    ));
                }
                if accepted_authorities.is_some() {
                    return Err(ApiError::invalid_request(
                        "accepted_authorities is only editable on managed VLESS endpoints",
                    ));
                }
                store.build_endpoint(
                    node_id,
                    crate::domain::EndpointKind::VlessRealityVisionTcp,
                    port,
                    json!({
                        "reality": reality,
                        "mihomo_smux": mihomo_smux,
                        "mihomo_xmux": mihomo_xmux,
                        "transport": transport
                    }),
                )?
            } else {
                let node = store.get_node(&node_id).ok_or_else(|| {
                    ApiError::invalid_request(format!("node not found: {node_id}"))
                })?;
                let reality =
                    managed_vless_reality_for_node(&state, &node, DEFAULT_VLESS_FINGERPRINT)?;
                let spec = DefaultVlessEndpointSpec {
                    port,
                    reality_dest: reality.dest,
                    server_names: reality.server_names,
                    server_names_source: reality.server_names_source,
                    fingerprint: reality.fingerprint,
                };
                let mut endpoint =
                    build_managed_default_vless_endpoint(&spec, node_id).map_err(|e| {
                        ApiError::invalid_request(format!(
                            "failed to create managed VLESS endpoint: {e}"
                        ))
                    })?;
                let mut meta: VlessRealityVisionTcpEndpointMeta =
                    serde_json::from_value(endpoint.meta.clone())
                        .map_err(|e| ApiError::internal(e.to_string()))?;
                meta.canary_upstream = canary_upstream.map(|mut upstream| {
                    upstream.url = upstream.url.trim().to_string();
                    upstream
                });
                meta.accepted_authorities =
                    normalize_accepted_authorities(accepted_authorities.as_deref().unwrap_or(&[]))
                        .map_err(|(reason, authority)| {
                            ApiError::invalid_request(format!(
                                "invalid accepted_authority: {authority} ({reason})"
                            ))
                        })?;
                meta.mihomo_smux = mihomo_smux;
                meta.mihomo_xmux = mihomo_xmux;
                meta.transport = transport;
                endpoint.meta =
                    serde_json::to_value(meta).map_err(|e| ApiError::internal(e.to_string()))?;
                endpoint
            }
        }
        CreateEndpointRequest::Ss2022_2022Blake3Aes128Gcm {
            node_id,
            port,
            canary_upstream,
            accepted_authorities,
            mihomo_smux,
        } => {
            if canary_upstream.is_some() {
                return Err(ApiError::invalid_request(
                    "canary_upstream is only supported for vless endpoints",
                ));
            }
            if accepted_authorities.is_some() {
                return Err(ApiError::invalid_request(
                    "accepted_authorities is only supported for managed vless endpoints",
                ));
            }
            let mihomo_smux = mihomo_smux.unwrap_or_default();
            mihomo_smux.validate().map_err(ApiError::invalid_request)?;
            let store = state.store.lock().await;
            store.build_endpoint(
                node_id,
                crate::domain::EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                port,
                json!({ "mihomo_smux": mihomo_smux }),
            )?
        }
    };
    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertEndpoint {
            endpoint: endpoint.clone(),
            expected: None,
        },
    )
    .await?;
    state.reconcile.request_full();
    Ok(Json(endpoint))
}

pub(super) async fn admin_internal_endpoint_mutation(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<EndpointMutationForwardRequest>,
) -> Result<(StatusCode, Json<Endpoint>), ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified.as_ref() else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if verified.context.route != InternalRoute::MeshV2
        || verified.context.target_id != state.cluster.node_id
    {
        return Err(ApiError::unauthorized("endpoint mutation route is invalid"));
    }
    if !is_current_member(&state, &verified.context.sender_id) {
        return Err(ApiError::unauthorized(
            "endpoint mutation sender is not a cluster member",
        ));
    }
    if !super::is_leader(&super::raft_metrics(&state)) {
        return Err(ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "endpoint mutation leader is no longer available",
        ));
    }
    let request_id = verified.context.request_id.clone();
    let idempotency_request = IdempotencyRequest {
        sender_id: verified.context.sender_id.clone(),
        semantic_sha256: verified.idempotency_sha256.clone(),
    };
    match state
        .internal_idempotency
        .begin(&request_id, &idempotency_request)
        .await
        .map_err(|error| ApiError::internal(format!("read endpoint mutation ledger: {error}")))?
    {
        IdempotencyBegin::Existing(stored) => {
            let endpoint = serde_json::from_value(stored.body).map_err(|error| {
                ApiError::internal(format!("decode stored endpoint mutation: {error}"))
            })?;
            return Ok((
                StatusCode::from_u16(stored.status).unwrap_or(StatusCode::OK),
                Json(endpoint),
            ));
        }
        IdempotencyBegin::InFlight => {
            return Err(ApiError::new(
                "raft_write_outcome_unknown",
                StatusCode::CONFLICT,
                "endpoint mutation is already in flight",
            ));
        }
        IdempotencyBegin::Mismatch => {
            return Err(ApiError::new(
                "idempotency_request_mismatch",
                StatusCode::CONFLICT,
                "internal request id is already bound to a different request",
            ));
        }
        IdempotencyBegin::Full => {
            return Err(ApiError::new(
                "idempotency_ledger_full",
                StatusCode::TOO_MANY_REQUESTS,
                "internal idempotency ledger is full; retry after active records expire",
            ));
        }
        IdempotencyBegin::New => {}
    }
    let result = match request {
        EndpointMutationForwardRequest::Create { request } => {
            admin_create_endpoint_local(state.clone(), request).await
        }
        EndpointMutationForwardRequest::Patch {
            endpoint_id,
            request,
        } => super::admin_patch_endpoint_local(state.clone(), endpoint_id, request).await,
    };
    let response = match result {
        Ok(Json(endpoint)) => endpoint,
        Err(error) => {
            if should_abort_endpoint_mutation_error(error.code()) {
                state
                    .internal_idempotency
                    .abort(&request_id, &idempotency_request)
                    .await
                    .map_err(|abort_error| {
                        ApiError::internal(format!("abort endpoint mutation ledger: {abort_error}"))
                    })?;
            }
            return Err(error);
        }
    };
    state
        .internal_idempotency
        .finish(
            &request_id,
            &idempotency_request,
            StoredResult {
                status: StatusCode::OK.as_u16(),
                body: serde_json::to_value(&response)
                    .map_err(|error| ApiError::internal(error.to_string()))?,
            },
        )
        .await
        .map_err(|error| {
            ApiError::new(
                "raft_write_outcome_unknown",
                StatusCode::BAD_GATEWAY,
                format!("endpoint mutation result could not be verified: {error}"),
            )
        })?;
    Ok((StatusCode::OK, Json(response)))
}

fn should_abort_endpoint_mutation_error(code: &str) -> bool {
    code != "raft_write_outcome_unknown"
}

fn create_contains_xmux(request: &CreateEndpointRequest) -> bool {
    matches!(
        request,
        CreateEndpointRequest::VlessRealityVisionTcp {
            mihomo_xmux: Some(_),
            ..
        }
    )
}

fn request_id_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .map(ToOwned::to_owned)
        .unwrap_or_else(crate::id::new_ulid_string)
}

fn is_current_member(state: &AppState, node_id: &str) -> bool {
    let node_id = match crate::raft::types::raft_node_id_from_ulid(node_id) {
        Ok(node_id) => node_id,
        Err(_) => return false,
    };
    super::raft_metrics(state)
        .membership_config
        .nodes()
        .any(|(id, _)| *id == node_id)
}

async fn leader_node(state: &AppState) -> Option<Node> {
    let leader_id = super::raft_metrics(state).current_leader?;
    let store = state.store.lock().await;
    store.list_nodes().into_iter().find(|node| {
        crate::raft::types::raft_node_id_from_ulid(&node.node_id)
            .ok()
            .is_some_and(|node_id| node_id == leader_id)
    })
}

pub(super) async fn forward_endpoint_mutation(
    state: &AppState,
    request: EndpointMutationForwardRequest,
    request_id: String,
) -> Result<Json<Endpoint>, ApiError> {
    let Some(node) = leader_node(state).await else {
        return Err(ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "no cluster leader is currently available",
        ));
    };
    if node.node_id == state.cluster.node_id {
        return Err(ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "cluster leadership changed; retry the endpoint mutation",
        ));
    }
    let body =
        serde_json::to_vec(&request).map_err(|error| ApiError::internal(error.to_string()))?;
    let response = super::mesh::send_mesh_internal_request_raw(
        state,
        &state.mesh_client,
        &node,
        Method::POST,
        "/api/admin/_internal/endpoints/mutation".to_owned(),
        body,
        Some("application/json".to_owned()),
        Duration::from_secs(12),
        false,
        request_id,
    )
    .await?;
    let response = response.map_err(map_mesh_forward_error)?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = response.bytes().await.map_err(|error| {
        ApiError::new(
            "raft_write_outcome_unknown",
            StatusCode::BAD_GATEWAY,
            format!("leader endpoint mutation response could not be read: {error}"),
        )
    })?;
    if !status.is_success() {
        return Err(forward_error(status, &body));
    }
    serde_json::from_slice(&body).map(Json).map_err(|error| {
        ApiError::new(
            "raft_write_outcome_unknown",
            StatusCode::BAD_GATEWAY,
            format!("leader endpoint mutation response was invalid: {error}"),
        )
    })
}

fn map_mesh_forward_error(error: MeshRequestError) -> ApiError {
    match error {
        MeshRequestError::PreDispatchAuth(_)
        | MeshRequestError::PreDispatchTimeout
        | MeshRequestError::InvalidTarget(_) => ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "cluster leader is unavailable",
        ),
        MeshRequestError::CircuitOpen {
            dispatched: false, ..
        } => ApiError::new(
            "leader_unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "cluster leader is unavailable",
        ),
        _ => ApiError::new(
            "raft_write_outcome_unknown",
            StatusCode::BAD_GATEWAY,
            "leader endpoint mutation result could not be verified",
        ),
    }
}

fn forward_error(status: StatusCode, body: &[u8]) -> ApiError {
    #[derive(Deserialize)]
    struct Envelope {
        error: ErrorBody,
    }
    #[derive(Deserialize)]
    struct ErrorBody {
        code: String,
        message: String,
        #[serde(default)]
        details: Map<String, Value>,
    }
    let Ok(envelope) = serde_json::from_slice::<Envelope>(body) else {
        return ApiError::new(
            "raft_write_outcome_unknown",
            StatusCode::BAD_GATEWAY,
            "leader returned an invalid endpoint mutation response",
        );
    };
    let raw_code = envelope.error.code.as_str();
    let code = match raw_code {
        "invalid_request" => "invalid_request",
        "not_found" => "not_found",
        "conflict" => "conflict",
        "revision_conflict" => "revision_conflict",
        "coordinated_upgrade_required" => "coordinated_upgrade_required",
        "leader_unavailable" => "leader_unavailable",
        "raft_write_not_dispatched" => "raft_write_not_dispatched",
        "raft_write_outcome_unknown" => "raft_write_outcome_unknown",
        "timeout" => "timeout",
        "internal" => "internal",
        _ => "raft_write_outcome_unknown",
    };
    let message = match raw_code {
        "invalid_request"
        | "not_found"
        | "conflict"
        | "revision_conflict"
        | "coordinated_upgrade_required"
        | "leader_unavailable"
        | "raft_write_not_dispatched"
        | "raft_write_outcome_unknown" => envelope.error.message,
        _ => "leader endpoint mutation failed; inspect the structured error details".to_owned(),
    };
    let details = envelope
        .error
        .details
        .into_iter()
        .filter(|(key, _)| {
            matches!(
                key.as_str(),
                "dispatch_state"
                    | "confidence"
                    | "retryable"
                    | "support_id"
                    | "target"
                    | "target_node_id"
                    | "remote_status"
                    | "manual_retry_required"
                    | "manual_retry_warning"
                    | "cause"
                    | "failure_layer"
                    | "attempted_path"
            )
        })
        .collect();
    ApiError::with_details(code, status, message, details)
}

#[cfg(test)]
mod tests {
    use super::should_abort_endpoint_mutation_error;

    #[test]
    fn unknown_raft_result_keeps_idempotency_pending() {
        assert!(!should_abort_endpoint_mutation_error(
            "raft_write_outcome_unknown"
        ));
        assert!(should_abort_endpoint_mutation_error("invalid_request"));
    }
}
