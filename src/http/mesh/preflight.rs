use super::*;
use crate::control_plane_mesh::MeshRequestError;

const MAX_MESH_PREFLIGHT_RESPONSE_BYTES: usize = 64 * 1024;
const MESH_PREFLIGHT_TOTAL_BUDGET: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeshPreflightRoute {
    Direct,
    RegisteredApi,
}

fn mesh_preflight_route(target: &MeshPeerTarget) -> MeshPreflightRoute {
    if target.mesh_base_url.is_some() {
        MeshPreflightRoute::Direct
    } else {
        MeshPreflightRoute::RegisteredApi
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MeshPreflightRequest {
    pub voter_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MeshPreflightFailureKind {
    InvalidTarget,
    Transport,
    Protocol,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MeshPreflightFailure {
    pub sender_node_id: String,
    pub target_node_id: String,
    pub kind: MeshPreflightFailureKind,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MeshPreflightResponse {
    pub sender_node_id: String,
    pub failures: Vec<MeshPreflightFailure>,
}

pub(crate) async fn admin_internal_mesh_preflight(
    Extension(state): Extension<AppState>,
    internal: Option<Extension<InternalSignatureAuth>>,
    ApiJson(request): ApiJson<MeshPreflightRequest>,
) -> Result<Json<MeshPreflightResponse>, ApiError> {
    let Some(Extension(internal)) = internal else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    let Some(verified) = internal.verified.as_ref() else {
        return Err(ApiError::unauthorized("internal auth required"));
    };
    if verified.context.route != internal_auth::InternalRoute::MeshV2
        || verified.context.target_id != state.cluster.node_id
    {
        return Err(ApiError::unauthorized("mesh preflight identity is invalid"));
    }
    let requested = request.voter_node_ids.into_iter().collect::<BTreeSet<_>>();
    match tokio::time::timeout(
        MESH_PREFLIGHT_TOTAL_BUDGET,
        run_internal_mesh_preflight(state.clone(), requested, verified.context.sender_id.clone()),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(ApiError::conflict("mesh preflight deadline exceeded")),
    }
}

async fn run_internal_mesh_preflight(
    state: AppState,
    requested: BTreeSet<String>,
    sender_node_id: String,
) -> Result<Json<MeshPreflightResponse>, ApiError> {
    let voter_node_ids = current_voter_node_ids(&state).await?;
    if requested != voter_node_ids || !requested.contains(&sender_node_id) {
        return Err(ApiError::conflict(
            "mesh preflight voter set changed; retry against current membership",
        ));
    }

    let mut failures = Vec::new();
    for target_node_id in requested {
        if target_node_id == state.cluster.node_id {
            continue;
        }
        let target = mesh_peer_target(&state, &target_node_id).await?;
        if let Err(error) = run_peer_health_preflight(&state, &target).await {
            failures.push(MeshPreflightFailure {
                sender_node_id: state.cluster.node_id.clone(),
                target_node_id,
                kind: classify_preflight_error(&error),
            });
        }
    }

    Ok(Json(MeshPreflightResponse {
        sender_node_id: state.cluster.node_id.clone(),
        failures,
    }))
}

pub(crate) async fn run_mesh_enable_preflight(
    state: AppState,
) -> Result<(), Vec<MeshPreflightFailure>> {
    match tokio::time::timeout(
        MESH_PREFLIGHT_TOTAL_BUDGET,
        run_mesh_enable_preflight_inner(state.clone()),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(vec![MeshPreflightFailure {
            sender_node_id: state.cluster.node_id.clone(),
            target_node_id: state.cluster.node_id.clone(),
            kind: MeshPreflightFailureKind::Transport,
        }]),
    }
}

async fn run_mesh_enable_preflight_inner(state: AppState) -> Result<(), Vec<MeshPreflightFailure>> {
    let voter_node_ids = current_voter_node_ids(&state).await.map_err(|_error| {
        vec![MeshPreflightFailure {
            sender_node_id: state.cluster.node_id.clone(),
            target_node_id: state.cluster.node_id.clone(),
            kind: MeshPreflightFailureKind::InvalidTarget,
        }]
    })?;
    let mut failures = Vec::new();

    for target_node_id in voter_node_ids.iter() {
        if target_node_id == &state.cluster.node_id {
            continue;
        }
        let target = match mesh_peer_target(&state, target_node_id).await {
            Ok(target) => target,
            Err(_) => {
                failures.push(MeshPreflightFailure {
                    sender_node_id: state.cluster.node_id.clone(),
                    target_node_id: target_node_id.clone(),
                    kind: MeshPreflightFailureKind::InvalidTarget,
                });
                continue;
            }
        };
        if let Err(error) = run_peer_health_preflight(&state, &target).await {
            failures.push(MeshPreflightFailure {
                sender_node_id: state.cluster.node_id.clone(),
                target_node_id: target_node_id.clone(),
                kind: classify_preflight_error(&error),
            });
        }
    }

    let body = serde_json::to_vec(&MeshPreflightRequest {
        voter_node_ids: voter_node_ids.iter().cloned().collect(),
    })
    .expect("mesh preflight request serializes");
    let mut remote_results = Vec::new();
    for target_node_id in voter_node_ids
        .iter()
        .filter(|node_id| node_id.as_str() != state.cluster.node_id.as_str())
    {
        remote_results.push(
            run_remote_preflight(&state, target_node_id, body.clone(), &voter_node_ids).await,
        );
    }
    for result in remote_results {
        match result {
            Ok(remote) => failures.extend(remote.failures),
            Err((target_node_id, kind)) => failures.push(MeshPreflightFailure {
                sender_node_id: state.cluster.node_id.clone(),
                target_node_id,
                kind,
            }),
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

async fn run_remote_preflight(
    state: &AppState,
    target_node_id: &str,
    body: Vec<u8>,
    expected_voters: &BTreeSet<String>,
) -> Result<MeshPreflightResponse, (String, MeshPreflightFailureKind)> {
    let mut target = mesh_peer_target(state, target_node_id).await.map_err(|_| {
        (
            target_node_id.to_string(),
            MeshPreflightFailureKind::InvalidTarget,
        )
    })?;
    // The gate is intentionally closed during preflight. Force this coordination request over
    // the registered Public Path; the remote node performs its own Direct-only checks.
    target.mesh_base_url = None;
    let ca_key_pem = state.cluster_ca_key_pem.as_deref().ok_or_else(|| {
        (
            target_node_id.to_string(),
            MeshPreflightFailureKind::Transport,
        )
    })?;
    let response = state
        .mesh_client
        .send_peer_request(
            &target,
            MeshRequest {
                method: Method::POST,
                path_and_query: "/api/admin/_internal/mesh/preflight".to_string(),
                content_type: Some("application/json".to_string()),
                body,
                total_budget: Duration::from_secs(10),
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
        .map_err(|error| (target_node_id.to_string(), classify_preflight_error(&error)))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MESH_PREFLIGHT_RESPONSE_BYTES as u64)
    {
        return Err((
            target_node_id.to_string(),
            MeshPreflightFailureKind::Protocol,
        ));
    }
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(MAX_MESH_PREFLIGHT_RESPONSE_BYTES as u64) as usize,
    );
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| {
            (
                target_node_id.to_string(),
                MeshPreflightFailureKind::Transport,
            )
        })?;
        if bytes.len().saturating_add(chunk.len()) > MAX_MESH_PREFLIGHT_RESPONSE_BYTES {
            return Err((
                target_node_id.to_string(),
                MeshPreflightFailureKind::Protocol,
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let response: MeshPreflightResponse = serde_json::from_slice(&bytes).map_err(|_| {
        (
            target_node_id.to_string(),
            MeshPreflightFailureKind::Protocol,
        )
    })?;
    if !validate_remote_preflight_response(&response, target_node_id, expected_voters) {
        return Err((
            target_node_id.to_string(),
            MeshPreflightFailureKind::Protocol,
        ));
    }
    Ok(response)
}

fn validate_remote_preflight_response(
    response: &MeshPreflightResponse,
    expected_sender: &str,
    expected_voters: &BTreeSet<String>,
) -> bool {
    response.sender_node_id == expected_sender
        && response.failures.len() <= expected_voters.len()
        && response.failures.iter().all(|failure| {
            failure.sender_node_id == expected_sender
                && failure.target_node_id != expected_sender
                && expected_voters.contains(&failure.target_node_id)
        })
}

/// Validate the route that this target is allowed to use when Mesh is enabled.
///
/// Nodes without a managed endpoint are the supported private-container
/// exception: they remain voters, but their signed control-plane path is the
/// registered API base URL. Endpoint-bearing nodes must still pass Direct Mesh
/// preflight and never use this public path as a substitute.
async fn run_peer_health_preflight(
    state: &AppState,
    target: &MeshPeerTarget,
) -> Result<(), MeshRequestError> {
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| MeshRequestError::InvalidTarget("cluster CA is unavailable".into()))?;
    let request = MeshRequest {
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
    };
    let result = match mesh_preflight_route(target) {
        MeshPreflightRoute::Direct => {
            state
                .mesh_client
                .send_peer_direct_preflight_for_reenable(
                    target,
                    request,
                    ca_key_pem,
                    &state.cluster_ca_pem,
                )
                .await
        }
        MeshPreflightRoute::RegisteredApi => {
            state
                .mesh_client
                .send_peer_request(target, request, ca_key_pem, &state.cluster_ca_pem)
                .await
        }
    };
    let response = result?;
    drop(response);
    Ok(())
}

fn classify_preflight_error(error: &MeshRequestError) -> MeshPreflightFailureKind {
    match error {
        MeshRequestError::InvalidTarget(_) => MeshPreflightFailureKind::InvalidTarget,
        MeshRequestError::Auth(_) | MeshRequestError::Protocol(_) => {
            MeshPreflightFailureKind::Protocol
        }
        MeshRequestError::CircuitOpen { .. }
        | MeshRequestError::OutcomeUnknown
        | MeshRequestError::Public(_)
        | MeshRequestError::ReverseTimeout
        | MeshRequestError::Reverse(_) => MeshPreflightFailureKind::Transport,
    }
}

async fn current_voter_node_ids(state: &AppState) -> Result<BTreeSet<String>, ApiError> {
    let metrics = raft_metrics(state);
    let voter_ids = metrics
        .membership_config
        .membership()
        .voter_ids()
        .collect::<BTreeSet<_>>();
    let nodes = {
        let store = state.store.lock().await;
        store.list_nodes()
    };
    let mut node_ids = BTreeSet::new();
    for node in nodes {
        let raft_id = crate::raft::types::raft_node_id_from_ulid(&node.node_id)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        if voter_ids.contains(&raft_id) {
            node_ids.insert(node.node_id);
        }
    }
    if node_ids.len() != voter_ids.len() {
        return Err(ApiError::conflict(
            "every current voter must have mapped node metadata for Mesh preflight",
        ));
    }
    Ok(node_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_telemetry::MeshPeerReason;

    #[test]
    fn private_voter_without_endpoint_uses_registered_api_preflight() {
        let target = MeshPeerTarget {
            node_id: xp_test_fixtures::tertiary_node_id().to_owned(),
            node_name: xp_test_fixtures::tertiary_node_name().to_owned(),
            mesh_base_url: xp_test_fixtures::none(),
            endpoint_transport: None,
            endpoint_fingerprint: None,
            mesh_reason: MeshPeerReason::MissingEndpoint,
            public_base_url: xp_test_fixtures::tertiary_api_url().to_owned(),
        };

        assert_eq!(
            mesh_preflight_route(&target),
            MeshPreflightRoute::RegisteredApi
        );
    }

    #[test]
    fn endpoint_bearing_voter_keeps_direct_preflight() {
        let target = MeshPeerTarget {
            node_id: xp_test_fixtures::secondary_node_id().to_owned(),
            node_name: xp_test_fixtures::secondary_node_name().to_owned(),
            mesh_base_url: Some(xp_test_fixtures::url_https_public_peer_afixture_test().to_owned()),
            endpoint_transport: Some("xhttp_reality_fallback"),
            endpoint_fingerprint: Some(
                xp_test_fixtures::mesh_fingerprint_primary_xhttp().to_owned(),
            ),
            mesh_reason: MeshPeerReason::MeshAvailable,
            public_base_url: xp_test_fixtures::secondary_api_url().to_owned(),
        };

        assert_eq!(mesh_preflight_route(&target), MeshPreflightRoute::Direct);
    }

    fn voters() -> BTreeSet<String> {
        ["node-a", "node-b", "node-c"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn remote_preflight_requires_expected_sender_and_targets() {
        let expected = voters();
        let response = MeshPreflightResponse {
            sender_node_id: "node-b".to_owned(),
            failures: vec![MeshPreflightFailure {
                sender_node_id: "node-b".to_owned(),
                target_node_id: "node-a".to_owned(),
                kind: MeshPreflightFailureKind::Transport,
            }],
        };
        assert!(validate_remote_preflight_response(
            &response, "node-b", &expected
        ));

        let mut wrong_sender = response.clone();
        wrong_sender.sender_node_id = "node-c".to_owned();
        assert!(!validate_remote_preflight_response(
            &wrong_sender,
            "node-b",
            &expected,
        ));

        let mut wrong_target = response;
        wrong_target.failures[0].target_node_id = "unknown".to_owned();
        assert!(!validate_remote_preflight_response(
            &wrong_target,
            "node-b",
            &expected,
        ));
    }

    #[test]
    fn remote_preflight_bounds_failure_count() {
        let expected = voters();
        let response = MeshPreflightResponse {
            sender_node_id: "node-b".to_owned(),
            failures: (0..=expected.len())
                .map(|_| MeshPreflightFailure {
                    sender_node_id: "node-b".to_owned(),
                    target_node_id: "node-a".to_owned(),
                    kind: MeshPreflightFailureKind::Protocol,
                })
                .collect(),
        };
        assert!(!validate_remote_preflight_response(
            &response, "node-b", &expected
        ));
    }
}
