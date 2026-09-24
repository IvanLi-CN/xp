use axum::http::StatusCode;

use super::ApiError;
use crate::control_plane_mesh::{MeshAwareHttpClient, MeshRequestError};

pub(super) async fn resource_mesh_error(
    client: &MeshAwareHttpClient,
    node_id: &str,
    support_id: String,
    error: MeshRequestError,
) -> ApiError {
    match error {
        MeshRequestError::CircuitOpen { path } => {
            let attempted_path = if path == "Public" { "public" } else { "direct" };
            let Some(retry_after) = client
                .circuits()
                .retry_after_seconds(node_id, attempted_path == "public")
                .await
            else {
                return ApiError::internal("resource circuit cooldown is unavailable")
                    .with_detail("failure_layer", "circuit_breaker")
                    .with_detail("cause", "cooldown_unavailable")
                    .with_detail("confidence", "unknown")
                    .with_detail("target_node_id", node_id)
                    .with_detail("attempted_path", attempted_path)
                    .with_detail("dispatch_state", "not_dispatched")
                    .with_detail("retryable", false)
                    .with_detail("support_id", support_id);
            };
            ApiError::new(
                "peer_circuit_open",
                StatusCode::SERVICE_UNAVAILABLE,
                "resource request is cooling down",
            )
            .with_detail("failure_layer", "circuit_breaker")
            .with_detail("cause", "circuit_open")
            .with_detail("confidence", "confirmed")
            .with_detail("target_node_id", node_id)
            .with_detail("attempted_path", attempted_path)
            .with_detail("dispatch_state", "not_dispatched")
            .with_detail("retryable", true)
            .with_detail("support_id", support_id)
            .with_retry_after_seconds(retry_after)
        }
        MeshRequestError::Protocol(_) => response(
            "peer_protocol_rejected",
            StatusCode::BAD_GATEWAY,
            "peer response could not be verified",
            "peer_protocol",
            "protocol_rejected",
            "confirmed",
            "unknown",
            "dispatched_no_verified_response",
            true,
            node_id,
            support_id,
        ),
        MeshRequestError::Auth(_) => response(
            "peer_protocol_rejected",
            StatusCode::BAD_GATEWAY,
            "peer authentication could not be verified",
            "peer_protocol",
            "peer_authentication_failed",
            "confirmed",
            "unknown",
            "dispatched_no_verified_response",
            false,
            node_id,
            support_id,
        ),
        MeshRequestError::Public(error) => {
            public_transport_error(error.is_timeout(), node_id, support_id)
        }
        MeshRequestError::OutcomeUnknown => response(
            "peer_transport_error",
            StatusCode::BAD_GATEWAY,
            "resource response could not be verified",
            "peer_transport",
            "outcome_unknown",
            "unknown",
            "unknown",
            "dispatched_no_verified_response",
            true,
            node_id,
            support_id,
        ),
        MeshRequestError::TransportTimeout => response(
            "peer_transport_timeout",
            StatusCode::GATEWAY_TIMEOUT,
            "target node did not return a verified response in time",
            "peer_transport",
            "peer_timeout",
            "confirmed",
            "unknown",
            "dispatched_no_verified_response",
            true,
            node_id,
            support_id,
        ),
        MeshRequestError::InvalidTarget(_) => response(
            "peer_target_invalid",
            StatusCode::BAD_GATEWAY,
            "the target node route is not usable",
            "peer_transport",
            "invalid_target",
            "confirmed",
            "none",
            "not_dispatched",
            false,
            node_id,
            support_id,
        ),
        MeshRequestError::ReverseTimeout => response(
            "peer_transport_timeout",
            StatusCode::GATEWAY_TIMEOUT,
            "target node did not return a verified response in time",
            "peer_transport",
            "peer_timeout",
            "confirmed",
            "reverse",
            "dispatched_no_verified_response",
            true,
            node_id,
            support_id,
        ),
        MeshRequestError::Reverse(_) => response(
            "peer_route_unavailable",
            StatusCode::BAD_GATEWAY,
            "the target node route is unavailable",
            "peer_transport",
            "route_unavailable",
            "unknown",
            "unknown",
            "not_dispatched",
            true,
            node_id,
            support_id,
        ),
    }
}

fn public_transport_error(is_timeout: bool, node_id: &str, support_id: String) -> ApiError {
    if is_timeout {
        response(
            "peer_transport_timeout",
            StatusCode::GATEWAY_TIMEOUT,
            "target node did not return a verified response in time",
            "peer_transport",
            "peer_timeout",
            "confirmed",
            "public",
            "dispatched_no_verified_response",
            true,
            node_id,
            support_id,
        )
    } else {
        response(
            "peer_transport_error",
            StatusCode::BAD_GATEWAY,
            "target node transport failed before a verified response",
            "peer_transport",
            "transport_error",
            "confirmed",
            "public",
            "dispatched_no_verified_response",
            true,
            node_id,
            support_id,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn response(
    code: &'static str,
    status: StatusCode,
    message: &'static str,
    layer: &'static str,
    cause: &'static str,
    confidence: &'static str,
    path: &'static str,
    dispatch: &'static str,
    retryable: bool,
    node_id: &str,
    support_id: String,
) -> ApiError {
    ApiError::new(code, status, message)
        .with_detail("failure_layer", layer)
        .with_detail("cause", cause)
        .with_detail("confidence", confidence)
        .with_detail("target_node_id", node_id)
        .with_detail("attempted_path", path)
        .with_detail("dispatch_state", dispatch)
        .with_detail("retryable", retryable)
        .with_detail("support_id", support_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> MeshAwareHttpClient {
        MeshAwareHttpClient::new(reqwest::Client::new())
    }

    fn assert_mapping(
        error: &ApiError,
        code: &str,
        status: StatusCode,
        dispatch: &str,
        retryable: bool,
    ) {
        assert_eq!(error.code, code);
        assert_eq!(error.status, status);
        assert_eq!(error.details["dispatch_state"], dispatch);
        assert_eq!(error.details["retryable"], retryable);
        assert_eq!(error.details["target_node_id"], "node-a");
        assert_eq!(error.details["support_id"], "support-a");
    }

    #[tokio::test]
    async fn resource_mesh_errors_keep_branch_contracts() {
        let client = test_client();
        let cases = [
            (
                MeshRequestError::Protocol("ignored".to_string()),
                "peer_protocol_rejected",
                StatusCode::BAD_GATEWAY,
                "dispatched_no_verified_response",
                true,
            ),
            (
                MeshRequestError::Auth(crate::internal_auth::AuthError::Invalid("ignored")),
                "peer_protocol_rejected",
                StatusCode::BAD_GATEWAY,
                "dispatched_no_verified_response",
                false,
            ),
            (
                MeshRequestError::OutcomeUnknown,
                "peer_transport_error",
                StatusCode::BAD_GATEWAY,
                "dispatched_no_verified_response",
                true,
            ),
            (
                MeshRequestError::TransportTimeout,
                "peer_transport_timeout",
                StatusCode::GATEWAY_TIMEOUT,
                "dispatched_no_verified_response",
                true,
            ),
            (
                MeshRequestError::InvalidTarget("ignored".to_string()),
                "peer_target_invalid",
                StatusCode::BAD_GATEWAY,
                "not_dispatched",
                false,
            ),
            (
                MeshRequestError::Reverse("ignored".to_string()),
                "peer_route_unavailable",
                StatusCode::BAD_GATEWAY,
                "not_dispatched",
                true,
            ),
            (
                MeshRequestError::ReverseTimeout,
                "peer_transport_timeout",
                StatusCode::GATEWAY_TIMEOUT,
                "dispatched_no_verified_response",
                true,
            ),
        ];

        for (mesh_error, code, status, dispatch, retryable) in cases {
            let mapped =
                resource_mesh_error(&client, "node-a", "support-a".to_string(), mesh_error).await;
            assert_mapping(&mapped, code, status, dispatch, retryable);
        }
    }

    #[tokio::test]
    async fn circuit_open_mapping_is_bounded_and_not_dispatched() {
        let client = test_client();
        let circuits = client.circuits();
        circuits.record_protocol_failure("node-a").await;
        let mapped = resource_mesh_error(
            &client,
            "node-a",
            "support-a".to_string(),
            MeshRequestError::CircuitOpen {
                path: "Direct Mesh",
            },
        )
        .await;

        assert_mapping(
            &mapped,
            "peer_circuit_open",
            StatusCode::SERVICE_UNAVAILABLE,
            "not_dispatched",
            true,
        );
        assert_eq!(mapped.details["failure_layer"], "circuit_breaker");
        assert!(
            mapped.details["retry_after_seconds"]
                .as_u64()
                .is_some_and(|seconds| (1..=300).contains(&seconds))
        );
    }

    #[tokio::test]
    async fn circuit_open_without_available_cooldown_is_internal() {
        let client = test_client();
        let circuits = client.circuits();
        {
            let mut peers = circuits.peers.lock().await;
            let circuit = peers.entry("node-a".to_string()).or_default();
            circuit.retry_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
            circuit.half_open_in_flight = true;
        }

        let mapped = resource_mesh_error(
            &client,
            "node-a",
            "support-a".to_string(),
            MeshRequestError::CircuitOpen {
                path: "Direct Mesh",
            },
        )
        .await;

        assert_eq!(mapped.code, "internal");
        assert_eq!(mapped.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(mapped.details["cause"], "cooldown_unavailable");
        assert!(mapped.details.get("retry_after_seconds").is_none());
    }

    #[test]
    fn public_transport_mapping_distinguishes_timeout() {
        let timeout = public_transport_error(true, "node-a", "support-a".to_string());
        assert_mapping(
            &timeout,
            "peer_transport_timeout",
            StatusCode::GATEWAY_TIMEOUT,
            "dispatched_no_verified_response",
            true,
        );

        let transport = public_transport_error(false, "node-a", "support-a".to_string());
        assert_mapping(
            &transport,
            "peer_transport_error",
            StatusCode::BAD_GATEWAY,
            "dispatched_no_verified_response",
            true,
        );
    }
}
