use super::*;
use crate::control_plane_mesh::{DirectValidationState, MeshRequestError};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MeshPreflightRequest {
    pub voter_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct MeshPreflightFailure {
    pub sender_node_id: String,
    pub target_node_id: String,
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    let voter_node_ids = current_voter_node_ids(&state).await?;
    let requested = request.voter_node_ids.into_iter().collect::<BTreeSet<_>>();
    if requested != voter_node_ids || !requested.contains(&verified.context.sender_id) {
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
        if let Err(error) = run_direct_health_preflight(&state, &target).await {
            failures.push(MeshPreflightFailure {
                sender_node_id: state.cluster.node_id.clone(),
                target_node_id,
                kind: classify_preflight_error(&error).to_string(),
            });
        }
    }
    Ok(Json(MeshPreflightResponse {
        sender_node_id: state.cluster.node_id.clone(),
        failures,
    }))
}

pub(crate) async fn run_mesh_enable_preflight(
    state: &AppState,
) -> Result<(), Vec<MeshPreflightFailure>> {
    let voter_node_ids = current_voter_node_ids(state).await.map_err(|_error| {
        vec![MeshPreflightFailure {
            sender_node_id: state.cluster.node_id.clone(),
            target_node_id: state.cluster.node_id.clone(),
            kind: "invalid_target".to_string(),
        }]
    })?;
    let mut failures = Vec::new();

    for target_node_id in voter_node_ids.iter() {
        if target_node_id == &state.cluster.node_id {
            continue;
        }
        let target = match mesh_peer_target(state, target_node_id).await {
            Ok(target) => target,
            Err(_error) => {
                failures.push(MeshPreflightFailure {
                    sender_node_id: state.cluster.node_id.clone(),
                    target_node_id: target_node_id.clone(),
                    kind: "invalid_target".to_string(),
                });
                continue;
            }
        };
        if let Err(error) = run_direct_health_preflight(state, &target).await {
            failures.push(MeshPreflightFailure {
                sender_node_id: state.cluster.node_id.clone(),
                target_node_id: target_node_id.clone(),
                kind: classify_preflight_error(&error).to_string(),
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
        remote_results.push(run_remote_preflight(state, target_node_id, body.clone()).await);
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
) -> Result<MeshPreflightResponse, (String, String)> {
    let mut target = mesh_peer_target(state, target_node_id)
        .await
        .map_err(|_| (target_node_id.to_string(), "invalid_target".to_string()))?;
    // The gate is intentionally closed during preflight. Force this coordination request over
    // the registered Public Path; the remote node performs its own Direct-only checks.
    target.mesh_base_url = None;
    let ca_key_pem = state.cluster_ca_key_pem.as_deref().ok_or_else(|| {
        (
            target_node_id.to_string(),
            "transport:cluster_ca_unavailable".into(),
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
        .map_err(|error| {
            (
                target_node_id.to_string(),
                classify_preflight_error(&error).into(),
            )
        })?;
    let bytes = response
        .bytes()
        .await
        .map_err(|_| (target_node_id.to_string(), "transport:response_body".into()))?;
    serde_json::from_slice(&bytes).map_err(|_| {
        (
            target_node_id.to_string(),
            "protocol:invalid_response".into(),
        )
    })
}

async fn run_direct_health_preflight(
    state: &AppState,
    target: &MeshPeerTarget,
) -> Result<(), MeshRequestError> {
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| MeshRequestError::InvalidTarget("cluster CA is unavailable".into()))?;
    let result = state
        .mesh_client
        .send_peer_direct_preflight(
            target,
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
        .await;
    let response = match result {
        Ok(response) => response,
        Err(error) => {
            let validation_state = match error {
                MeshRequestError::Auth(_) | MeshRequestError::Protocol(_) => {
                    DirectValidationState::ProtocolRejected
                }
                _ => DirectValidationState::TransportFailed,
            };
            state
                .mesh_client
                .mark_direct_validation_failure(target, validation_state)
                .await;
            return Err(error);
        }
    };
    drop(response);
    state
        .mesh_client
        .circuits()
        .record_success(&target.node_id)
        .await;
    state
        .mesh_client
        .mark_direct_validation_success(target)
        .await;
    Ok(())
}

fn classify_preflight_error(error: &MeshRequestError) -> &'static str {
    match error {
        MeshRequestError::InvalidTarget(_) => "invalid_target",
        MeshRequestError::Auth(_) | MeshRequestError::Protocol(_) => "protocol",
        MeshRequestError::CircuitOpen { .. }
        | MeshRequestError::OutcomeUnknown
        | MeshRequestError::Public(_)
        | MeshRequestError::Reverse(_) => "transport",
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
