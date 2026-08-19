use super::*;
use crate::http::join_capability::require_reverse_assignment_on_voters;
use sha2::{Digest, Sha256};
#[path = "mesh/bootstrap.rs"]
mod bootstrap;
#[path = "mesh/status.rs"]
mod status;

#[derive(Debug, Clone, Serialize)]
struct AdminMeshStatusResponse {
    generated_at: String,
    revision: u64,
    local: AdminMeshLocalStatus,
    peers: Vec<AdminMeshPeerStatus>,
    events: Vec<crate::mesh_telemetry::MeshTelemetryEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminMeshLocalStatus {
    node_id: String,
    node_name: String,
    cluster_id: String,
    role: String,
    leader_api_base_url: String,
    term: u64,
    canary: crate::vless_https_canary::VlessHttpsCanaryStatus,
}

#[derive(Debug, Clone, Serialize)]
struct AdminMeshPeerStatus {
    node_id: String,
    node_name: String,
    api_base_url: String,
    mesh_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_capability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_reason: Option<crate::mesh_telemetry::MeshPeerReason>,
    current_path: Option<TelemetryPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_route: Option<crate::mesh_telemetry::MeshActiveRoute>,
    quality: MeshQuality,
    stale: bool,
    breaker: BreakerState,
    last_sample_at: Option<String>,
    last_transition_at: Option<String>,
    availability_1h: Option<f64>,
    availability_24h: Option<f64>,
    mesh_availability_24h: Option<f64>,
    latency_p50_ms: Option<u32>,
    latency_p95_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_transport: Option<AdminMeshTransportStatus>,
    buckets: Vec<crate::mesh_telemetry::MeshTelemetryBucket>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminMeshTransportStatus {
    protocol: Option<MeshTransportProtocol>,
    health: MeshTransportHealth,
    connection_generation: u64,
    current_connection_requests: u64,
    requests_5m: u32,
    connection_starts_5m: u32,
    requests_1h: u32,
    connection_starts_1h: u32,
    last_connection_started_at: Option<String>,
}

pub(super) async fn admin_internal_reverse_relay(
    Extension(state): Extension<AppState>,
    headers: HeaderMap,
    internal: Option<Extension<InternalSignatureAuth>>,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(outer) = internal.verified.as_ref() else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let envelope = crate::reverse_mesh::ReverseRelayEnvelope::from_headers(&headers)
        .map_err(ApiError::invalid_request)?;
    if outer.context.route != internal_auth::InternalRoute::MeshV2
        || outer.context.target_id != state.cluster.node_id
        || (outer.context.sender_id == state.cluster.node_id
            && envelope.target_node_id == state.cluster.node_id)
    {
        return Err(ApiError::unauthorized(
            "reverse relay outer route is invalid",
        ));
    }
    if !state.reconcile.reverse_gate().load(Ordering::Acquire) {
        return Err(ApiError::conflict(
            "reverse relay is disabled until local Xray readiness recovers",
        ));
    }
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
    let relay_route = match envelope.route.as_str() {
        route if route == internal_auth::InternalRoute::MeshV2.as_str() => {
            internal_auth::InternalRoute::MeshV2
        }
        route if route == internal_auth::InternalRoute::HealthV2.as_str() => {
            internal_auth::InternalRoute::HealthV2
        }
        _ => {
            return Err(ApiError::unauthorized(
                "reverse relay inner route is not permitted",
            ));
        }
    };
    if !envelope.verify(ca_key_pem)
        || envelope.sender_node_id != outer.context.sender_id
        || envelope.request_id != outer.context.request_id
    {
        return Err(ApiError::unauthorized("reverse relay envelope is invalid"));
    }
    if envelope.content_length != body.len()
        || body.len() > crate::reverse_mesh::relay_body_limit(&envelope.uri)
        || envelope.uri.contains("/mesh/reverse-relay")
    {
        return Err(ApiError::invalid_request(
            "reverse relay body or route is invalid",
        ));
    }
    let method = Method::from_bytes(envelope.method.as_bytes())
        .map_err(|_| ApiError::invalid_request("reverse relay method is invalid"))?;
    let uri = envelope
        .uri
        .parse::<axum::http::Uri>()
        .map_err(|_| ApiError::invalid_request("reverse relay URI is invalid"))?;
    if !(uri.path().starts_with("/api/admin/_internal/") || uri.path().starts_with("/raft/")) {
        return Err(ApiError::unauthorized(
            "reverse relay path is not permitted",
        ));
    }
    if relay_route == internal_auth::InternalRoute::HealthV2
        && (method != Method::GET
            || uri.path() != "/api/admin/_internal/mesh/health"
            || !body.is_empty())
    {
        return Err(ApiError::unauthorized(
            "reverse health relay must be a bodyless health GET",
        ));
    }

    let current_voter_ids = raft_metrics(&state)
        .membership_config
        .membership()
        .voter_ids()
        .collect::<BTreeSet<_>>();
    let current_learner_ids = raft_metrics(&state)
        .membership_config
        .membership()
        .learner_ids()
        .collect::<BTreeSet<_>>();
    let is_current_voter = |node_id: &str| {
        raft_node_id_from_ulid(node_id)
            .ok()
            .is_some_and(|raft_id| current_voter_ids.contains(&raft_id))
    };
    let is_current_learner = |node_id: &str| {
        raft_node_id_from_ulid(node_id)
            .ok()
            .is_some_and(|raft_id| current_learner_ids.contains(&raft_id))
    };
    if !is_current_voter(&state.cluster.node_id) || !is_current_voter(&envelope.sender_node_id) {
        return Err(ApiError::conflict(
            "reverse relay requires a current voter rendezvous and sender",
        ));
    }
    let bootstrap_route =
        relay_route == internal_auth::InternalRoute::HealthV2 || uri.path().starts_with("/raft/");
    let bootstrap_learner_target = is_current_learner(&envelope.target_node_id) && bootstrap_route;

    let (assignment, role, reverse_epoch) = {
        let store = state.store.lock().await;
        let Some(assignment) = store
            .state()
            .reverse_mesh_assignments
            .get(&envelope.target_node_id)
            .cloned()
        else {
            return Err(ApiError::conflict(
                "reverse relay assignment is unavailable",
            ));
        };
        if !assignment.is_valid()
            || assignment.generation != envelope.assignment_generation
            || assignment.credential_epoch != store.state().reverse_mesh_epoch
            || (!is_current_voter(&assignment.target_node_id) && !bootstrap_learner_target)
            || !is_current_voter(&assignment.primary_node_id)
            || assignment
                .standby_node_id
                .as_deref()
                .is_some_and(|node_id| !is_current_voter(node_id))
            || store.get_node(&envelope.target_node_id).is_none()
            || store.get_node(&envelope.sender_node_id).is_none()
        {
            return Err(ApiError::conflict("reverse relay assignment is stale"));
        }
        let role = bootstrap::reverse_role_for_relay(
            store.state().active_membership_operation(),
            &assignment,
            &state.cluster.node_id,
            &envelope.target_node_id,
            bootstrap_route,
        )
        .ok_or_else(|| {
            ApiError::unauthorized("this node is not the assigned reverse Rendezvous")
        })?;
        (assignment, role, store.state().reverse_mesh_epoch)
    };

    if relay_route != internal_auth::InternalRoute::HealthV2
        && !bootstrap_learner_target
        && !state
            .reverse_relay
            .has_health_verified(&assignment.target_node_id, assignment.generation)
            .await
    {
        return Err(ApiError::conflict(
            "reverse relay generation is awaiting signed health verification",
        ));
    }

    let mut inner_headers = HeaderMap::new();
    if !envelope.content_type.is_empty() {
        inner_headers.insert(
            header::CONTENT_TYPE,
            envelope
                .content_type
                .parse()
                .map_err(|_| ApiError::invalid_request("reverse relay content type is invalid"))?,
        );
    }
    inner_headers.insert(
        header::CONTENT_LENGTH,
        envelope
            .content_length
            .to_string()
            .parse()
            .map_err(|_| ApiError::invalid_request("reverse relay content length is invalid"))?,
    );
    let issued_at = envelope.issued_at.to_string();
    for (name, value) in [
        (
            internal_auth::INTERNAL_ROUTE_HEADER,
            envelope.route.as_str(),
        ),
        (
            internal_auth::INTERNAL_CLUSTER_ID_HEADER,
            outer.context.cluster_id.as_str(),
        ),
        (
            internal_auth::INTERNAL_SENDER_ID_HEADER,
            envelope.sender_node_id.as_str(),
        ),
        (
            internal_auth::INTERNAL_TARGET_ID_HEADER,
            envelope.target_node_id.as_str(),
        ),
        (
            internal_auth::INTERNAL_REQUEST_ID_HEADER,
            envelope.request_id.as_str(),
        ),
        (internal_auth::INTERNAL_ISSUED_AT_HEADER, issued_at.as_str()),
        (
            internal_auth::INTERNAL_SIGNATURE_HEADER,
            envelope.inner_signature.as_str(),
        ),
    ] {
        inner_headers.insert(
            header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ApiError::invalid_request("reverse relay header is invalid"))?,
            value
                .parse()
                .map_err(|_| ApiError::invalid_request("reverse relay header value is invalid"))?,
        );
    }
    let verified_inner = internal_auth::verify_request_v2(
        ca_key_pem,
        &state.cluster_ca_pem,
        &method,
        &uri,
        &inner_headers,
        &body,
        &outer.context.cluster_id,
        &envelope.target_node_id,
    )
    .map_err(|_| ApiError::unauthorized("reverse relay inner signature is invalid"))?;
    if verified_inner.context.sender_id != outer.context.sender_id
        || verified_inner.context.request_id != outer.context.request_id
    {
        return Err(ApiError::unauthorized(
            "reverse relay inner identity mismatch",
        ));
    }

    // The in-memory replay window is fast-path protection; the existing local idempotency ledger
    // makes the same signed request fail closed across an XP restart without storing request body
    // or response content. A relay response cannot safely be replayed once its stream has started.
    let idempotency_request = IdempotencyRequest {
        sender_id: envelope.sender_node_id.clone(),
        semantic_sha256: outer.idempotency_sha256.clone(),
    };
    match state
        .internal_idempotency
        .begin(&envelope.request_id, &idempotency_request)
        .await
        .map_err(|error| ApiError::internal(format!("read reverse relay replay ledger: {error}")))?
    {
        IdempotencyBegin::New => {}
        IdempotencyBegin::Existing(_) | IdempotencyBegin::InFlight | IdempotencyBegin::Mismatch => {
            return Err(ApiError::new(
                "outcome_unknown",
                StatusCode::CONFLICT,
                "reverse relay request was already accepted",
            ));
        }
        IdempotencyBegin::Full => {
            return Err(ApiError::new(
                "idempotency_ledger_full",
                StatusCode::TOO_MANY_REQUESTS,
                "reverse relay replay ledger is full; retry after records expire",
            ));
        }
    }
    state
        .internal_idempotency
        .finish(
            &envelope.request_id,
            &idempotency_request,
            StoredResult {
                status: StatusCode::ACCEPTED.as_u16(),
                body: json!({"accepted": true}),
            },
        )
        .await
        .map_err(|error| {
            ApiError::internal(format!("persist reverse relay replay ledger: {error}"))
        })?;
    if !state
        .reverse_relay
        .accept_request(&envelope.sender_node_id, &envelope.request_id)
        .await
    {
        return Err(ApiError::new(
            "outcome_unknown",
            StatusCode::CONFLICT,
            "reverse relay request was already accepted",
        ));
    }

    let password = crate::reverse_mesh::derive_reverse_password(
        ca_key_pem,
        &state.cluster.node_id,
        reverse_epoch,
    );
    let origin_id = crate::reverse_mesh::derive_reverse_origin_id(
        reverse_epoch,
        &assignment.target_node_id,
        &state.cluster.node_id,
        role,
        assignment.generation,
    );
    let origin = crate::reverse_mesh::derive_reverse_origin(&origin_id);
    let response = state
        .reverse_relay
        .forward(
            crate::reverse_mesh::REVERSE_PORTAL_ADDRESS,
            crate::reverse_mesh::REVERSE_SOCKS_USERNAME,
            &password,
            &origin,
            method,
            uri.path_and_query()
                .map(|value| value.as_str())
                .unwrap_or(uri.path()),
            &inner_headers,
            body.to_vec(),
            Duration::from_secs(5),
        )
        .await
        .map_err(|error| {
            ApiError::new(
                "reverse_relay_unavailable",
                StatusCode::SERVICE_UNAVAILABLE,
                error.to_string(),
            )
        })?;
    let status = response.status();
    let inner_ack = response
        .headers()
        .get(internal_auth::INTERNAL_ACK_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::gateway_timeout("reverse target acknowledgement is missing"))?;
    internal_auth::verify_ack_v2(
        ca_key_pem,
        &state.cluster_ca_pem,
        &verified_inner,
        &assignment.target_node_id,
        status.as_u16(),
        inner_ack,
    )
    .map_err(|_| ApiError::gateway_timeout("reverse target acknowledgement is invalid"))?;
    if relay_route == internal_auth::InternalRoute::HealthV2 {
        state
            .reverse_relay
            .mark_health_verified(&assignment.target_node_id, assignment.generation)
            .await;
    }
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = response.headers().get(header::CONTENT_TYPE) {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    builder = builder.header(
        header::HeaderName::from_static(crate::reverse_mesh::RELAY_INNER_ACK_HEADER),
        inner_ack,
    );
    let stream = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    builder
        .body(Body::from_stream(stream))
        .map_err(|_| ApiError::internal("build reverse relay response"))
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct AdminMeshProbeRequest {
    #[serde(default)]
    node_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct AdminMeshProbeResponse {
    accepted_node_ids: Vec<String>,
    revision: u64,
}

pub(super) fn spawn_mesh_probe_worker(state: AppState) {
    tokio::spawn(async move {
        // Probe evidence is local operational telemetry, not replicated cluster state.
        let jitter_secs = u64::from(rand::random::<u8>() % 10);
        tokio::time::sleep(Duration::from_secs(jitter_secs)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        let mut probe_round = 0_u64;
        loop {
            interval.tick().await;
            probe_round = probe_round.wrapping_add(1);
            let probe_public_standby = probe_round % 5 == 1;
            let node_ids = {
                let store = state.store.lock().await;
                store
                    .list_nodes()
                    .into_iter()
                    .filter(|node| node.node_id != state.cluster.node_id)
                    .map(|node| node.node_id)
                    .collect::<Vec<_>>()
            };
            let probe_gate = state.mesh_telemetry.probe_gate();
            stream::iter(node_ids)
                .map(|node_id| {
                    let state = state.clone();
                    let probe_gate = probe_gate.clone();
                    async move {
                        let Ok(_permit) = probe_gate.acquire_owned().await else {
                            return;
                        };
                        if let Err(error) = probe_mesh_peer(&state, &node_id).await {
                            tracing::debug!(
                                peer_id = %node_id,
                                ?error,
                                "scheduled mesh probe failed"
                            );
                        }
                        if probe_public_standby
                            && let Err(error) = probe_mesh_public_standby(&state, &node_id).await
                        {
                            tracing::debug!(
                                peer_id = %node_id,
                                ?error,
                                "scheduled public standby probe failed"
                            );
                        }
                    }
                })
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await;
        }
    });
}

#[derive(Debug, Deserialize)]
struct ReverseCapabilityResponse {
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    reverse_mesh: Option<capabilities::ReverseMeshReadiness>,
}

pub(super) async fn reverse_candidate_readiness(
    state: &AppState,
    node: &Node,
) -> Result<bool, ApiError> {
    let managed_vless_endpoint = {
        let store = state.store.lock().await;
        store.list_endpoints().into_iter().any(|endpoint| {
            endpoint.node_id == node.node_id
                && crate::managed_default_endpoints::managed_default_vless_endpoint(&endpoint)
                    .is_some()
        })
    };
    if !managed_vless_endpoint {
        return Ok(false);
    }
    if node.node_id == state.cluster.node_id {
        return Ok(
            matches!(state.xray_health.snapshot().await.status, XrayStatus::Up)
                && state.reconcile.reverse_gate().load(Ordering::Acquire),
        );
    }
    let response = match send_mesh_internal_capability_read(
        state,
        &state.mesh_client,
        node,
        Duration::from_secs(5),
    )
    .await?
    {
        MeshCapabilityProbeResponse::Verified(response) => response,
        MeshCapabilityProbeResponse::PredecessorNotFound => return Ok(false),
    };
    let body = response
        .json::<ReverseCapabilityResponse>()
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(body
        .reverse_mesh
        .as_ref()
        .is_some_and(|readiness| readiness.reverse_ready))
}

/// The leader owns Reverse assignment orchestration. Runtime links remain local; only the epoch
/// and deterministic assignment are replicated through the normal Raft command path.
pub(super) fn spawn_reverse_assignment_worker(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut failures = BTreeMap::<String, ReverseAssignmentFailure>::new();
        loop {
            interval.tick().await;
            if let Err(error) = reconcile_reverse_assignments(&state, &mut failures).await {
                tracing::debug!(?error, "reverse assignment reconciliation skipped");
            }
        }
    });
}

#[derive(Debug)]
struct ReverseAssignmentFailure {
    consecutive: u8,
    next_attempt: StdInstant,
}

async fn reconcile_reverse_assignments(
    state: &AppState,
    failures: &mut BTreeMap<String, ReverseAssignmentFailure>,
) -> Result<(), ApiError> {
    let metrics = raft_metrics(state);
    if !is_leader(&metrics) {
        return Ok(());
    }
    let voter_ids = metrics
        .membership_config
        .membership()
        .voter_ids()
        .collect::<std::collections::BTreeSet<_>>();
    if voter_ids.len() < 2 {
        return Ok(());
    }
    let (nodes, endpoints, current_epoch, current_assignments, generation_counters) = {
        let store = state.store.lock().await;
        (
            store.list_nodes(),
            store.list_endpoints(),
            store.state().reverse_mesh_epoch,
            store.state().reverse_mesh_assignments.clone(),
            store.state().reverse_mesh_generation_counters.clone(),
        )
    };
    // Do not establish the durable epoch until every current voter understands the assignment
    // command and the relay status surface. Once an epoch exists, an unavailable voter is treated
    // as a failed candidate so an already assigned standby can take over after backoff.
    if current_epoch == 0 {
        require_reverse_assignment_on_voters(state).await?;
    }
    let voter_node_ids = nodes
        .iter()
        .filter_map(|node| {
            crate::raft::types::raft_node_id_from_ulid(&node.node_id)
                .ok()
                .filter(|raft_id| voter_ids.contains(raft_id))
                .map(|_| node.node_id.clone())
        })
        .collect::<Vec<_>>();
    if voter_node_ids.len() != voter_ids.len() {
        return Err(ApiError::conflict(
            "reverse assignment requires every voter to map to DesiredState",
        ));
    }
    let epoch = if current_epoch != 0 {
        current_epoch
    } else {
        reverse_membership_revision(&metrics)
    };

    let mut candidates = Vec::new();
    for node in &nodes {
        if !voter_node_ids.iter().any(|id| id == &node.node_id) {
            continue;
        }
        let managed_vless = endpoints.iter().any(|endpoint| {
            endpoint.node_id == node.node_id
                && crate::managed_default_endpoints::managed_default_vless_endpoint(endpoint)
                    .is_some()
        });
        let (capabilities, signed_xray_ready) = if node.node_id == state.cluster.node_id {
            (
                vec![
                    crate::reverse_mesh::REVERSE_ASSIGNMENT_CAPABILITY.to_string(),
                    crate::reverse_mesh::REVERSE_RELAY_CAPABILITY.to_string(),
                ],
                matches!(state.xray_health.snapshot().await.status, XrayStatus::Up)
                    && state.reconcile.reverse_gate().load(Ordering::Acquire),
            )
        } else {
            let response = send_mesh_internal_capability_read(
                state,
                &state.mesh_client,
                node,
                Duration::from_secs(5),
            )
            .await;
            let response = match response {
                Ok(response) => response,
                Err(error) if current_epoch != 0 => {
                    candidates.push(crate::reverse_mesh::ReverseMeshCandidate {
                        node_id: node.node_id.clone(),
                        assignment_capable: false,
                        relay_capable: false,
                        signed_xray_ready: false,
                        managed_vless_endpoint: managed_vless,
                    });
                    tracing::debug!(node_id = %node.node_id, ?error, "reverse capability probe unavailable");
                    continue;
                }
                Err(error) => return Err(ApiError::gateway_timeout(error.message)),
            };
            let response = match response {
                MeshCapabilityProbeResponse::Verified(response) => response,
                MeshCapabilityProbeResponse::PredecessorNotFound => {
                    return Err(ApiError::conflict(
                        "reverse assignment requires signed capability support on every voter",
                    ));
                }
            };
            let body = match response.json::<ReverseCapabilityResponse>().await {
                Ok(body) => body,
                Err(error) if current_epoch != 0 => {
                    candidates.push(crate::reverse_mesh::ReverseMeshCandidate {
                        node_id: node.node_id.clone(),
                        assignment_capable: false,
                        relay_capable: false,
                        signed_xray_ready: false,
                        managed_vless_endpoint: managed_vless,
                    });
                    tracing::debug!(node_id = %node.node_id, ?error, "reverse capability response invalid");
                    continue;
                }
                Err(error) => return Err(ApiError::gateway_timeout(error.to_string())),
            };
            let signed_xray_ready = body
                .reverse_mesh
                .as_ref()
                .is_some_and(|readiness| readiness.reverse_ready);
            (body.capabilities, signed_xray_ready)
        };
        candidates.push(crate::reverse_mesh::ReverseMeshCandidate {
            node_id: node.node_id.clone(),
            assignment_capable: capabilities
                .iter()
                .any(|cap| cap == crate::reverse_mesh::REVERSE_ASSIGNMENT_CAPABILITY),
            relay_capable: capabilities
                .iter()
                .any(|cap| cap == crate::reverse_mesh::REVERSE_RELAY_CAPABILITY),
            signed_xray_ready: managed_vless && signed_xray_ready,
            managed_vless_endpoint: managed_vless,
        });
    }
    // Keep a currently assigned Rendezvous through the first two validation failures. This
    // avoids generation churn on transient Xray restarts; the third failure rotates the link.
    let now = StdInstant::now();
    let mut selection_candidates = candidates.clone();
    let mut forced_targets = BTreeSet::new();
    for (target, assignment) in &current_assignments {
        for rendezvous in assignment.rendezvous_ids() {
            let Some(candidate) = candidates.iter().find(|item| item.node_id == rendezvous) else {
                continue;
            };
            let validation_failed = candidate.managed_vless_endpoint
                && !candidate.signed_xray_ready
                && (candidate.assignment_capable && candidate.relay_capable
                    || assignment.contains_rendezvous(rendezvous));
            let key = format!("{target}\n{rendezvous}");
            if !validation_failed {
                failures.remove(&key);
                continue;
            }
            let failure = failures.entry(key).or_insert(ReverseAssignmentFailure {
                consecutive: 0,
                next_attempt: now,
            });
            if now >= failure.next_attempt {
                let attempt = usize::from(failure.consecutive);
                failure.consecutive = failure.consecutive.saturating_add(1);
                failure.next_attempt = now + crate::reverse_mesh::reverse_backoff(attempt);
            }
            if failure.consecutive >= 3 {
                forced_targets.insert(target.clone());
            } else if let Some(selection_candidate) = selection_candidates
                .iter_mut()
                .find(|item| item.node_id == rendezvous)
            {
                selection_candidate.assignment_capable = true;
                selection_candidate.relay_capable = true;
                selection_candidate.signed_xray_ready = true;
            }
        }
    }
    failures.retain(|key, _| {
        key.split_once('\n')
            .is_some_and(|(target, _)| current_assignments.contains_key(target))
    });
    let mut current_for_selection = current_assignments.clone();
    for target in &forced_targets {
        current_for_selection.remove(target);
    }
    let mut assigned = crate::reverse_mesh::assign_reverse_mesh_with_generation_floors(
        voter_node_ids.clone(),
        &selection_candidates,
        &current_for_selection,
        &generation_counters,
        reverse_membership_revision(&metrics),
        epoch,
    );
    for target in &forced_targets {
        if let (Some(current), Some(next)) =
            (current_assignments.get(target), assigned.get_mut(target))
        {
            next.generation =
                crate::reverse_mesh::reverse_mesh_generation_after_failures(current.generation, 3)
                    .unwrap_or_else(|| current.generation.saturating_add(1).max(1));
        }
    }
    if current_epoch == 0
        && candidates
            .iter()
            .any(crate::reverse_mesh::ReverseMeshCandidate::eligible)
    {
        state
            .raft
            .client_write(crate::state::DesiredStateCommand::SetReverseMeshEpoch { epoch })
            .await
            .map_err(|error| ApiError::internal(error.to_string()))?;
    }
    for target in &voter_node_ids {
        match (current_assignments.get(target), assigned.get(target)) {
            (Some(current), Some(next)) if current == next => {}
            (current, Some(next)) => {
                state
                    .raft
                    .client_write(
                        crate::state::DesiredStateCommand::UpsertReverseMeshAssignment {
                            assignment: next.clone(),
                            expected_generation: current.map(|item| item.generation),
                        },
                    )
                    .await
                    .map_err(|error| ApiError::internal(error.to_string()))?;
            }
            (Some(current), None) => {
                state
                    .raft
                    .client_write(
                        crate::state::DesiredStateCommand::DeleteReverseMeshAssignment {
                            target_node_id: target.clone(),
                            expected_generation: Some(current.generation),
                        },
                    )
                    .await
                    .map_err(|error| ApiError::internal(error.to_string()))?;
            }
            (None, None) => {}
        }
    }
    verify_reverse_assignments(state, &assigned).await;
    Ok(())
}

async fn verify_reverse_assignments(
    state: &AppState,
    assignments: &BTreeMap<String, crate::reverse_mesh::ReverseMeshAssignment>,
) {
    let target_ids = assignments.keys().cloned().collect::<Vec<_>>();
    let state_for_probes = state.clone();
    stream::iter(target_ids)
        .map(|target_id| {
            let state = state_for_probes.clone();
            async move {
                if let Err(error) = verify_reverse_assignment(&state, &target_id).await {
                    tracing::debug!(
                        target_id = %target_id,
                        ?error,
                        "signed reverse health probe failed"
                    );
                }
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
}

async fn verify_reverse_assignment(state: &AppState, target_id: &str) -> Result<(), ApiError> {
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
    let target = mesh_peer_target(state, target_id).await?;
    configure_reverse_route(state, &state.mesh_client, &target).await;
    state
        .mesh_client
        .send_peer_reverse_health_request(
            &target,
            crate::control_plane_mesh::MeshRequest {
                method: Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_string(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(5),
                allow_ambiguous_fallback: true,
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
    Ok(())
}

fn reverse_membership_revision(metrics: &openraft::RaftMetrics<RaftNodeId, RaftNodeMeta>) -> u64 {
    let revision = crate::raft_membership_guard::membership_revision(metrics).unwrap_or_default();
    let digest = Sha256::digest(revision.as_bytes());
    u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("membership digest has eight bytes"),
    )
    .max(1)
}

pub(super) async fn admin_internal_raft_client_write(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(cmd): ApiJson<DesiredStateCommand>,
) -> Result<Json<RaftClientResponse>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if matches!(
        cmd,
        DesiredStateCommand::UpsertNode { .. }
            | DesiredStateCommand::DeleteNode { .. }
            | DesiredStateCommand::BeginMembershipOperation { .. }
            | DesiredStateCommand::TransitionMembershipOperation { .. }
            | DesiredStateCommand::PruneMembershipOperations { .. }
    ) {
        return Err(ApiError::not_implemented(
            "node and membership lifecycle commands require dedicated lifecycle endpoints",
        ));
    }
    if matches!(
        &cmd,
        DesiredStateCommand::SetReverseMeshEpoch { .. }
            | DesiredStateCommand::UpsertReverseMeshAssignment { .. }
            | DesiredStateCommand::DeleteReverseMeshAssignment { .. }
    ) {
        crate::http::join_capability::require_reverse_assignment_on_voters(&state).await?;
    }
    let idempotency_request = internal
        .verified
        .as_ref()
        .filter(|verified| verified.context.route == internal_auth::InternalRoute::MeshV2)
        .map(|verified| {
            (
                verified.context.request_id.clone(),
                IdempotencyRequest {
                    sender_id: verified.context.sender_id.clone(),
                    semantic_sha256: verified.idempotency_sha256.clone(),
                },
            )
        });
    if let Some((request_id, request)) = idempotency_request.as_ref() {
        match state
            .internal_idempotency
            .begin(request_id, request)
            .await
            .map_err(|error| {
                ApiError::internal(format!("read internal idempotency ledger: {error}"))
            })? {
            IdempotencyBegin::Existing(stored) => {
                let response = serde_json::from_value(stored.body).map_err(|error| {
                    ApiError::internal(format!("decode stored internal result: {error}"))
                })?;
                return Ok(Json(response));
            }
            IdempotencyBegin::InFlight => {
                return Err(ApiError::new(
                    "outcome_unknown",
                    StatusCode::CONFLICT,
                    "internal mutation pending",
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
    }
    let resp = state
        .raft
        .client_write(cmd)
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some((request_id, request)) = idempotency_request {
        state
            .internal_idempotency
            .finish(
                &request_id,
                &request,
                StoredResult {
                    status: StatusCode::OK.as_u16(),
                    body: serde_json::to_value(&resp).map_err(|error| {
                        ApiError::internal(format!("encode internal idempotency result: {error}"))
                    })?,
                },
            )
            .await
            .map_err(|error| {
                ApiError::internal(format!("persist internal idempotency result: {error}"))
            })?;
    }
    Ok(Json(resp))
}

async fn build_admin_mesh_status_response(state: &AppState) -> AdminMeshStatusResponse {
    let now = Utc::now();
    let telemetry = state.mesh_telemetry.snapshot().await;
    let telemetry_by_peer = telemetry
        .peers
        .iter()
        .map(|peer| (peer.peer_id.as_str(), peer))
        .collect::<BTreeMap<_, _>>();
    let (nodes, endpoints, assignments) = {
        let store = state.store.lock().await;
        (
            store.list_nodes(),
            store.list_endpoints(),
            store.state().reverse_mesh_assignments.clone(),
        )
    };
    let peers = nodes
        .into_iter()
        .filter(|node| node.node_id != state.cluster.node_id)
        .map(|node| {
            let target = crate::control_plane_mesh::peer_target_from_node(&node, &endpoints);
            let mesh_url = target.mesh_base_url.clone();
            let mesh_enabled = mesh_url.is_some();
            let peer = telemetry_by_peer.get(node.node_id.as_str()).copied();
            let mesh_reason = if mesh_enabled {
                Some(
                    peer.and_then(|peer| peer.last_mesh_reason)
                        .filter(|_| {
                            peer.and_then(|peer| peer.last_mesh_target.as_deref())
                                == target.mesh_base_url.as_deref()
                        })
                        .unwrap_or(crate::mesh_telemetry::MeshPeerReason::NoSample),
                )
            } else {
                Some(target.mesh_reason)
            };
            let (
                availability_1h,
                availability_24h,
                mesh_availability_24h,
                latency_p50_ms,
                latency_p95_ms,
            ) = peer.map_or((None, None, None, None, None), |peer| {
                let (p50, p95) = latency_percentiles_for(peer, 24 * 60, now);
                (
                    availability_for(peer, 60, now),
                    availability_for(peer, 24 * 60, now),
                    mesh_availability_for(peer, now),
                    p50,
                    p95,
                )
            });
            AdminMeshPeerStatus {
                node_id: node.node_id.clone(),
                node_name: node.node_name,
                api_base_url: node.api_base_url,
                mesh_url,
                mesh_capability: Some(
                    if mesh_enabled { "enabled" } else { "disabled" }.to_string(),
                ),
                mesh_reason,
                current_path: peer.and_then(|peer| peer.last_path),
                active_route: status::with_assignment(
                    peer.and_then(|peer| peer.active_route.clone()),
                    assignments.get(&node.node_id),
                ),
                quality: peer.map_or(MeshQuality::Unknown, |peer| quality_for_peer(peer, now)),
                stale: is_mesh_peer_stale(peer, now),
                breaker: breaker_for_mesh_target(mesh_enabled, peer.and_then(|peer| peer.breaker)),
                last_sample_at: peer.and_then(|peer| peer.last_sample_at.clone()),
                last_transition_at: peer.and_then(|peer| peer.last_transition_at.clone()),
                availability_1h,
                availability_24h,
                mesh_availability_24h,
                latency_p50_ms,
                latency_p95_ms,
                mesh_transport: status::mesh_transport_status_for(mesh_enabled, peer, now),
                buckets: peer.map_or_else(Vec::new, |peer| {
                    crate::mesh_telemetry::buckets_for_last_24_hours(peer, now)
                }),
            }
        })
        .collect();
    let metrics = raft_metrics(state);
    AdminMeshStatusResponse {
        generated_at: telemetry.generated_at,
        revision: telemetry.revision,
        local: AdminMeshLocalStatus {
            node_id: state.cluster.node_id.clone(),
            node_name: state.cluster.node_name.clone(),
            cluster_id: state.cluster.cluster_id.clone(),
            role: if is_leader(&metrics) {
                "leader".to_string()
            } else {
                "follower".to_string()
            },
            leader_api_base_url: leader_api_base_url(&metrics).unwrap_or_default(),
            term: metrics.current_term,
            canary: crate::vless_https_canary::load_status(
                &state.config.data_dir,
                state.config.vless_canary_bind,
            ),
        },
        peers,
        events: telemetry.events,
    }
}

fn breaker_for_mesh_target(mesh_enabled: bool, recorded: Option<BreakerState>) -> BreakerState {
    if mesh_enabled {
        recorded.unwrap_or(BreakerState::Closed)
    } else {
        BreakerState::Disabled
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn disabled_mesh_target_never_reports_an_active_breaker() {
        assert_eq!(
            breaker_for_mesh_target(false, Some(BreakerState::Open)),
            BreakerState::Disabled
        );
        assert_eq!(breaker_for_mesh_target(false, None), BreakerState::Disabled);
        assert_eq!(breaker_for_mesh_target(true, None), BreakerState::Closed);
    }

    #[test]
    fn mesh_availability_uses_only_the_last_24_hours() {
        let now = xp_test_fixtures::baseline_timestamp()
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();
        let peer = crate::mesh_telemetry::MeshPeerTelemetry {
            buckets: std::collections::VecDeque::from([
                crate::mesh_telemetry::MeshTelemetryBucket {
                    minute: xp_test_fixtures::timestamp_at20231230_t230000_z().to_owned(),
                    mesh_success: 1,
                    ..Default::default()
                },
                crate::mesh_telemetry::MeshTelemetryBucket {
                    minute: xp_test_fixtures::baseline_timestamp().to_owned(),
                    mesh_failure: 1,
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };

        assert_eq!(mesh_availability_for(&peer, now), Some(0.0));
    }

    #[test]
    fn mesh_transport_status_is_optional_and_preserves_unknown_state() {
        let now = Utc::now();
        assert!(status::mesh_transport_status_for(false, None, now).is_none());

        let unknown = status::mesh_transport_status_for(true, None, now).unwrap();
        assert_eq!(unknown.health, MeshTransportHealth::Unknown);
        assert_eq!(unknown.protocol, None);
        assert_eq!(unknown.connection_generation, 0);

        let peer = crate::mesh_telemetry::MeshPeerTelemetry {
            last_mesh_protocol: Some(MeshTransportProtocol::H2),
            connection_generation: 1,
            current_connection_requests: 12,
            buckets: std::collections::VecDeque::from([
                crate::mesh_telemetry::MeshTelemetryBucket {
                    minute: now.to_rfc3339(),
                    mesh_h2_requests: 12,
                    mesh_connection_starts: 1,
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        let healthy = status::mesh_transport_status_for(true, Some(&peer), now).unwrap();
        assert_eq!(healthy.health, MeshTransportHealth::Healthy);
        assert_eq!(healthy.requests_5m, 12);
        assert_eq!(healthy.connection_starts_5m, 1);
    }

    #[test]
    fn mesh_status_etag_tracks_reuse_evidence_but_not_generation_time() {
        fn response(generated_at: &str, connection_starts_5m: u32) -> AdminMeshStatusResponse {
            AdminMeshStatusResponse {
                generated_at: generated_at.to_string(),
                revision: 7,
                local: AdminMeshLocalStatus {
                    node_id: "local".to_string(),
                    node_name: "local".to_string(),
                    cluster_id: "cluster".to_string(),
                    role: "leader".to_string(),
                    leader_api_base_url: "https://local.example.test".to_string(),
                    term: 3,
                    canary: crate::vless_https_canary::VlessHttpsCanaryStatus::disabled(
                        std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                    ),
                },
                peers: vec![AdminMeshPeerStatus {
                    node_id: "peer".to_string(),
                    node_name: "peer".to_string(),
                    api_base_url: "https://peer.example.test".to_string(),
                    mesh_url: Some("https://peer.example.test:443".to_string()),
                    mesh_capability: Some("enabled".to_string()),
                    mesh_reason: Some(crate::mesh_telemetry::MeshPeerReason::MeshAvailable),
                    current_path: Some(TelemetryPath::Mesh),
                    active_route: None,
                    quality: MeshQuality::Good,
                    stale: false,
                    breaker: BreakerState::Closed,
                    last_sample_at: None,
                    last_transition_at: None,
                    availability_1h: Some(1.0),
                    availability_24h: Some(1.0),
                    mesh_availability_24h: Some(1.0),
                    latency_p50_ms: Some(10),
                    latency_p95_ms: Some(20),
                    mesh_transport: Some(AdminMeshTransportStatus {
                        protocol: Some(MeshTransportProtocol::H2),
                        health: MeshTransportHealth::Healthy,
                        connection_generation: 2,
                        current_connection_requests: 12,
                        requests_5m: 12,
                        connection_starts_5m,
                        requests_1h: 60,
                        connection_starts_1h: 2,
                        last_connection_started_at: None,
                    }),
                    buckets: Vec::new(),
                }],
                events: Vec::new(),
            }
        }

        let first = response("2026-08-08T10:00:00Z", 1);
        let generated_later = response("2026-08-08T10:01:00Z", 1);
        let churning = response("2026-08-08T10:01:00Z", 3);

        assert_eq!(mesh_status_etag(&first), mesh_status_etag(&generated_later));
        assert_ne!(mesh_status_etag(&first), mesh_status_etag(&churning));
    }
}

fn mesh_availability_for(
    peer: &crate::mesh_telemetry::MeshPeerTelemetry,
    now: DateTime<Utc>,
) -> Option<f64> {
    let from = now - chrono::Duration::hours(24);
    let (success, total) = peer
        .buckets
        .iter()
        .filter_map(|bucket| {
            DateTime::parse_from_rfc3339(&bucket.minute)
                .ok()
                .map(|at| (bucket, at))
        })
        .filter(|(_, at)| at.with_timezone(&Utc) >= from)
        .fold((0u32, 0u32), |(success, total), (bucket, _)| {
            (
                success + bucket.mesh_success,
                total + bucket.mesh_success + bucket.mesh_failure,
            )
        });
    (total > 0).then_some(success as f64 / total as f64)
}

fn is_mesh_peer_stale(
    peer: Option<&crate::mesh_telemetry::MeshPeerTelemetry>,
    now: DateTime<Utc>,
) -> bool {
    peer.and_then(|peer| peer.last_sample_at.as_deref())
        .and_then(|sample| DateTime::parse_from_rfc3339(sample).ok())
        .is_some_and(|sample| {
            now.signed_duration_since(sample.with_timezone(&Utc)) > chrono::Duration::minutes(3)
        })
}

pub(super) async fn admin_get_mesh_status(
    Extension(state): Extension<AppState>,
    headers: HeaderMap,
) -> Response {
    let snapshot = build_admin_mesh_status_response(&state).await;
    let etag = mesh_status_etag(&snapshot);
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == etag)
    {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
    }
    (StatusCode::OK, [(header::ETAG, etag)], Json(snapshot)).into_response()
}

fn mesh_status_etag(snapshot: &AdminMeshStatusResponse) -> String {
    let mut stable_snapshot = snapshot.clone();
    stable_snapshot.generated_at.clear();
    let stable_bytes = serde_json::to_vec(&stable_snapshot).expect("serialize mesh status ETag");
    format!("\"mesh-{}\"", hex::encode(Sha256::digest(stable_bytes)))
}

pub(super) async fn admin_run_mesh_probes(
    Extension(state): Extension<AppState>,
    ApiJson(request): ApiJson<AdminMeshProbeRequest>,
) -> Result<Json<AdminMeshProbeResponse>, ApiError> {
    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let accepted_node_ids = if request.node_ids.is_empty() {
        nodes
            .into_iter()
            .filter(|node| node.node_id != state.cluster.node_id)
            .map(|node| node.node_id)
            .collect::<Vec<_>>()
    } else {
        if request.node_ids.len() > 50 {
            return Err(ApiError::invalid_request(
                "at most 50 mesh probe targets are allowed",
            ));
        }
        let known = nodes
            .into_iter()
            .map(|node| node.node_id)
            .collect::<BTreeSet<_>>();
        if request.node_ids.iter().collect::<BTreeSet<_>>().len() != request.node_ids.len()
            || request
                .node_ids
                .iter()
                .any(|id| !known.contains(id) || id == &state.cluster.node_id)
        {
            return Err(ApiError::invalid_request(
                "mesh probe contains an unknown or local node",
            ));
        }
        request.node_ids
    };
    for node_id in &accepted_node_ids {
        state
            .mesh_telemetry
            .record_event(
                node_id,
                "probe_requested",
                "operator requested an immediate mesh probe",
            )
            .await
            .map_err(|error| ApiError::internal(format!("persist mesh probe request: {error}")))?;
    }
    let state_for_probes = state.clone();
    let probe_node_ids = accepted_node_ids.clone();
    let probe_gate = state.mesh_telemetry.probe_gate();
    tokio::spawn(async move {
        stream::iter(probe_node_ids)
            .map(|node_id| {
                let state = state_for_probes.clone();
                let probe_gate = probe_gate.clone();
                async move {
                    let Ok(_permit) = probe_gate.acquire_owned().await else {
                        return;
                    };
                    if let Err(error) = probe_mesh_peer(&state, &node_id).await {
                        tracing::debug!(
                            peer_id = %node_id,
                            ?error,
                            "immediate mesh probe did not complete"
                        );
                    }
                }
            })
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await;
    });
    let revision = state.mesh_telemetry.snapshot().await.revision;
    Ok(Json(AdminMeshProbeResponse {
        accepted_node_ids,
        revision,
    }))
}

async fn mesh_peer_target(state: &AppState, node_id: &str) -> Result<MeshPeerTarget, ApiError> {
    let (node, endpoints) = {
        let store = state.store.lock().await;
        (
            store.get_node(node_id),
            store
                .list_endpoints()
                .into_iter()
                .filter(|endpoint| endpoint.node_id == node_id)
                .collect::<Vec<_>>(),
        )
    };
    let node = node.ok_or_else(|| ApiError::not_found(format!("node not found: {node_id}")))?;
    Ok(crate::control_plane_mesh::peer_target_from_node(
        &node, &endpoints,
    ))
}

async fn configure_reverse_route(
    state: &AppState,
    client: &MeshAwareHttpClient,
    target: &MeshPeerTarget,
) {
    let route = {
        let store = state.store.lock().await;
        (|| {
            let assignment = store
                .state()
                .reverse_mesh_assignments
                .get(&target.node_id)
                .cloned()?;
            let rendezvous_id = assignment.primary_node_id.clone();
            let rendezvous = store.get_node(&rendezvous_id)?;
            let standby_rendezvous = assignment
                .standby_node_id
                .as_deref()
                .and_then(|standby_id| store.get_node(standby_id))
                .map(|standby| {
                    let endpoints = store
                        .list_endpoints()
                        .into_iter()
                        .filter(|endpoint| endpoint.node_id == standby.node_id)
                        .collect::<Vec<_>>();
                    crate::control_plane_mesh::peer_target_from_node(&standby, &endpoints)
                });
            let endpoints = store
                .list_endpoints()
                .into_iter()
                .filter(|endpoint| endpoint.node_id == rendezvous_id)
                .collect::<Vec<_>>();
            let rendezvous_target =
                crate::control_plane_mesh::peer_target_from_node(&rendezvous, &endpoints);
            let role = if rendezvous_id == assignment.primary_node_id {
                crate::reverse_mesh::ReverseRole::Primary
            } else {
                crate::reverse_mesh::ReverseRole::Standby
            };
            Some(crate::control_plane_mesh::ReverseRelayRoute {
                rendezvous: rendezvous_target,
                standby_rendezvous,
                assignment,
                role,
            })
        })()
    };
    match route {
        Some(route) => {
            client
                .set_reverse_route(target.node_id.clone(), route)
                .await
        }
        None => client.clear_reverse_route(&target.node_id).await,
    }
}

pub(super) async fn send_mesh_internal_read(
    state: &AppState,
    client: &MeshAwareHttpClient,
    node: &Node,
    path_and_query: String,
    budget: Duration,
) -> Result<reqwest::Response, ApiError> {
    send_mesh_internal_request(
        state,
        client,
        node,
        Method::GET,
        path_and_query,
        Vec::new(),
        None,
        budget,
        true,
        crate::id::new_ulid_string(),
    )
    .await
}

/// Reads the one predecessor-compatible capability route.
pub(super) enum MeshCapabilityProbeResponse {
    Verified(reqwest::Response),
    PredecessorNotFound,
}

pub(super) async fn send_mesh_internal_capability_read(
    state: &AppState,
    client: &MeshAwareHttpClient,
    node: &Node,
    budget: Duration,
) -> Result<MeshCapabilityProbeResponse, ApiError> {
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
    let peer = mesh_peer_target(state, &node.node_id).await?;
    configure_reverse_route(state, client, &peer).await;
    let request = MeshRequest {
        method: Method::GET,
        path_and_query: "/api/admin/_internal/capabilities".to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: budget,
        allow_ambiguous_fallback: false,
        request_id: crate::id::new_ulid_string(),
        route: internal_auth::InternalRoute::MeshV2,
        cluster_id: state.cluster.cluster_id.clone(),
        sender_id: state.cluster.node_id.clone(),
        updates_active_path: true,
    };
    let response = if matches!(
        peer.mesh_reason,
        crate::mesh_telemetry::MeshPeerReason::MissingEndpoint
    ) {
        // A voter without a VLESS/REALITY Mesh endpoint must still prove capability through its
        // registered control-plane origin using the same Mesh-v2 request signature and
        // acknowledgement checks; a Mesh-capable peer never takes this path.
        client
            .send_peer_direct_request(
                &peer,
                crate::control_plane_mesh::PeerDirectPath::ApiBaseUrl,
                request,
                ca_key_pem,
                &state.cluster_ca_pem,
            )
            .await
            .map(crate::control_plane_mesh::CapabilityProbeResponse::Verified)
    } else {
        client
            .send_peer_request_allowing_legacy_not_found(
                &peer,
                request,
                ca_key_pem,
                &state.cluster_ca_pem,
            )
            .await
    }
    .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(match response {
        crate::control_plane_mesh::CapabilityProbeResponse::Verified(response) => {
            MeshCapabilityProbeResponse::Verified(response)
        }
        crate::control_plane_mesh::CapabilityProbeResponse::PredecessorNotFound => {
            MeshCapabilityProbeResponse::PredecessorNotFound
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn send_mesh_internal_request(
    state: &AppState,
    client: &MeshAwareHttpClient,
    node: &Node,
    method: Method,
    path_and_query: String,
    body: Vec<u8>,
    content_type: Option<String>,
    budget: Duration,
    allow_ambiguous_fallback: bool,
    request_id: String,
) -> Result<reqwest::Response, ApiError> {
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
    let peer = mesh_peer_target(state, &node.node_id).await?;
    configure_reverse_route(state, client, &peer).await;
    let request = MeshRequest {
        method,
        path_and_query,
        content_type,
        body,
        total_budget: budget,
        allow_ambiguous_fallback,
        request_id,
        route: internal_auth::InternalRoute::MeshV2,
        cluster_id: state.cluster.cluster_id.clone(),
        sender_id: state.cluster.node_id.clone(),
        updates_active_path: true,
    };
    client
        .send_peer_request(&peer, request, ca_key_pem, &state.cluster_ca_pem)
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))
}

pub(super) async fn probe_mesh_peer(state: &AppState, node_id: &str) -> Result<(), ApiError> {
    run_mesh_health_probe(state, node_id, false).await
}

pub(super) async fn probe_mesh_public_standby(
    state: &AppState,
    node_id: &str,
) -> Result<(), ApiError> {
    run_mesh_health_probe(state, node_id, true).await
}

async fn run_mesh_health_probe(
    state: &AppState,
    node_id: &str,
    public_only: bool,
) -> Result<(), ApiError> {
    let Some(ca_key_pem) = state.cluster_ca_key_pem.as_deref() else {
        return Err(ApiError::internal("cluster CA key is not available"));
    };
    let mut peer = mesh_peer_target(state, node_id).await?;
    if public_only {
        peer.mesh_base_url = None;
    }
    let client = state.mesh_client.clone();
    client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_string(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(5),
                allow_ambiguous_fallback: true,
                request_id: crate::id::new_ulid_string(),
                route: internal_auth::InternalRoute::HealthV2,
                cluster_id: state.cluster.cluster_id.clone(),
                sender_id: state.cluster.node_id.clone(),
                updates_active_path: !public_only,
            },
            ca_key_pem,
            &state.cluster_ca_pem,
        )
        .await
        .map_err(|error| ApiError::gateway_timeout(error.to_string()))?;
    Ok(())
}
