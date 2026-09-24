use super::*;
use crate::control_plane_mesh::MeshRequestFailure;

#[derive(Debug, Serialize)]
struct ResourceDiagnosticNode {
    node_id: String,
    node_name: String,
}

#[derive(Debug, Serialize)]
struct ResourcePeerDiagnostic {
    origin: ResourceDiagnosticNode,
    target: ResourceDiagnosticNode,
    route_attempts: Vec<crate::mesh_telemetry::MeshRouteAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_public_failure: Option<crate::mesh_telemetry::MeshPublicFailure>,
    public_circuit: crate::mesh_telemetry::BreakerState,
    request_id: String,
}

fn resource_peer_unavailable(
    state: &AppState,
    node: &Node,
    failure: MeshRequestFailure,
) -> ApiError {
    resource_peer_unavailable_for(
        ResourceDiagnosticNode {
            node_id: state.cluster.node_id.clone(),
            node_name: state.cluster.node_name.clone(),
        },
        ResourceDiagnosticNode {
            node_id: node.node_id.clone(),
            node_name: node.node_name.clone(),
        },
        failure.diagnostics,
    )
}

fn resource_peer_unavailable_for(
    origin: ResourceDiagnosticNode,
    target: ResourceDiagnosticNode,
    diagnostics: crate::control_plane_mesh::MeshRequestDiagnostics,
) -> ApiError {
    let diagnostic = ResourcePeerDiagnostic {
        origin,
        target,
        route_attempts: diagnostics.route_attempts,
        last_public_failure: diagnostics.last_public_failure,
        public_circuit: diagnostics
            .public_circuit
            .unwrap_or(crate::mesh_telemetry::BreakerState::Closed),
        request_id: diagnostics.request_id,
    };
    let mut error = ApiError::new(
        "resource_peer_unavailable",
        StatusCode::GATEWAY_TIMEOUT,
        "resource snapshot is unavailable from the target node",
    );
    error.details.insert(
        "diagnostic".to_string(),
        serde_json::to_value(diagnostic).expect("resource diagnostic is serializable"),
    );
    error
}

pub(crate) async fn send_mesh_internal_resource_read(
    state: &AppState,
    client: &MeshAwareHttpClient,
    node: &Node,
    path_and_query: String,
    budget: Duration,
) -> Result<reqwest::Response, ApiError> {
    let ca_key_pem = state
        .cluster_ca_key_pem
        .as_deref()
        .ok_or_else(|| ApiError::internal("cluster CA key is not available"))?;
    let peer = mesh_peer_target(state, &node.node_id).await?;
    let request = MeshRequest {
        method: Method::GET,
        path_and_query,
        content_type: None,
        body: Vec::new(),
        total_budget: budget,
        allow_ambiguous_fallback: true,
        request_id: crate::id::new_ulid_string(),
        route: internal_auth::InternalRoute::MeshV2,
        cluster_id: state.cluster.cluster_id.clone(),
        sender_id: state.cluster.node_id.clone(),
        updates_active_path: true,
    };
    client
        .send_peer_request_with_diagnostics(&peer, request, ca_key_pem, &state.cluster_ca_pem)
        .await
        .map(|(response, _)| response)
        .map_err(|failure| resource_peer_unavailable(state, node, failure))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_peer_error_contains_only_the_bounded_diagnostic_contract() {
        let diagnostics = crate::control_plane_mesh::MeshRequestDiagnostics {
            request_id: "01M0C1SJ5M1JWE6CCKMXNXPZ78".to_owned(),
            route_attempts: vec![crate::mesh_telemetry::MeshRouteAttempt {
                route: crate::mesh_telemetry::MeshRouteKind::DirectMesh,
                failure: crate::mesh_telemetry::MeshFailureClass::PreResponseTimeout,
                acknowledgement: crate::mesh_telemetry::MeshAcknowledgementState::NotObserved,
                dispatch: crate::mesh_telemetry::MeshDispatchState::DispatchedNoVerifiedResponse,
                observed_at: "2026-09-24T00:00:00Z".to_owned(),
                request_id: "01M0C1SJ5M1JWE6CCKMXNXPZ78".to_owned(),
                elapsed_ms: 5_000,
                retry_count: 1,
                http_status: None,
            }],
            public_circuit: Some(crate::mesh_telemetry::BreakerState::Open),
            last_public_failure: None,
        };
        let error = resource_peer_unavailable_for(
            ResourceDiagnosticNode {
                node_id: "node-101".to_owned(),
                node_name: "101".to_owned(),
            },
            ResourceDiagnosticNode {
                node_id: "node-us".to_owned(),
                node_name: "us".to_owned(),
            },
            diagnostics,
        );

        assert_eq!(error.code, "resource_peer_unavailable");
        assert_eq!(error.status, StatusCode::GATEWAY_TIMEOUT);
        let diagnostic = error.details["diagnostic"].as_object().unwrap();
        assert!(diagnostic.contains_key("origin"));
        assert!(diagnostic.contains_key("target"));
        assert!(diagnostic.contains_key("route_attempts"));
        assert!(diagnostic.contains_key("public_circuit"));
        assert!(diagnostic.contains_key("request_id"));
        assert!(!diagnostic.contains_key("url"));
        assert!(!diagnostic.contains_key("signature"));
        assert!(!diagnostic.contains_key("ack"));
    }
}
