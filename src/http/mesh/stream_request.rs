use super::*;

const RUNTIME_EVENTS_STREAM_LEASE: Duration = Duration::from_secs(15 * 60);

pub(crate) async fn send_mesh_internal_stream_read(
    state: &AppState,
    client: &MeshAwareHttpClient,
    node: &Node,
    path_and_query: String,
    budget: Duration,
) -> Result<reqwest::Response, ApiError> {
    let response = send_mesh_internal_request_raw_with_body_lease(
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
        Some(RUNTIME_EVENTS_STREAM_LEASE),
    )
    .await?;
    response.map_err(|error| ApiError::gateway_timeout(error.to_string()))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn send_mesh_internal_request_raw(
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
) -> Result<Result<reqwest::Response, MeshRequestError>, ApiError> {
    send_mesh_internal_request_raw_with_body_lease(
        state,
        client,
        node,
        method,
        path_and_query,
        body,
        content_type,
        budget,
        allow_ambiguous_fallback,
        request_id,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn send_mesh_internal_request_raw_with_body_lease(
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
    body_lease: Option<Duration>,
) -> Result<Result<reqwest::Response, MeshRequestError>, ApiError> {
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
    let peer = mesh_peer_target(state, &node.node_id).await?;
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
    let response = match body_lease {
        Some(body_lease) => {
            client
                .send_peer_request_with_body_lease(
                    &peer,
                    request,
                    ca_key_pem,
                    &state.cluster_ca_pem,
                    body_lease,
                )
                .await
        }
        None => {
            client
                .send_peer_request(&peer, request, ca_key_pem, &state.cluster_ca_pem)
                .await
        }
    };
    Ok(response)
}
