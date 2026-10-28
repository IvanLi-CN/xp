use crate::raft::app::{RaftWriteDispatchState, RaftWriteError};
use axum::{
    http::{StatusCode, header},
    response::IntoResponse,
};

#[tokio::test]
async fn retry_after_is_bounded_and_serialized() {
    let response = crate::http::ApiError::new(
        "peer_circuit_open",
        StatusCode::SERVICE_UNAVAILABLE,
        "resource request is cooling down",
    )
    .with_detail("failure_layer", "circuit_breaker")
    .with_retry_after_seconds(999)
    .into_response();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::RETRY_AFTER], "300");
    let body = super::body_json(response).await;
    assert_eq!(body["error"]["code"], "peer_circuit_open");
    assert_eq!(body["error"]["details"]["retry_after_seconds"], 300);
    assert_eq!(body["error"]["details"]["failure_layer"], "circuit_breaker");
}

#[test]
fn raft_write_api_error_exposes_only_safe_outcome_details() {
    let error = RaftWriteError::outcome_unknown(
        "response_decode",
        RaftWriteDispatchState::DispatchedNoVerifiedResponse,
        Some("node-a".to_string()),
        "support-a",
    )
    .with_remote_status(502);
    let api_error = super::super::map_raft_write_error(anyhow::Error::new(error));

    assert_eq!(api_error.code, "raft_write_outcome_unknown");
    assert_eq!(api_error.status, StatusCode::BAD_GATEWAY);
    assert_eq!(
        api_error.details["dispatch_state"],
        "dispatched_no_verified_response"
    );
    assert_eq!(api_error.details["target_node_id"], "node-a");
    assert_eq!(api_error.details["support_id"], "support-a");
    assert_eq!(api_error.details["remote_status"], 502);
    assert!(!api_error.message.contains("response_decode"));
}

#[test]
fn raft_write_not_dispatched_omits_overwrite_warning() {
    let error = RaftWriteError::not_dispatched(
        "remote_not_dispatched",
        Some("node-a".to_string()),
        "support-a",
    )
    .with_remote_status(503);
    let api_error = super::super::map_raft_write_error(anyhow::Error::new(error));

    assert_eq!(api_error.code, "raft_write_not_dispatched");
    assert_eq!(api_error.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(api_error.details["dispatch_state"], "not_dispatched");
    assert_eq!(api_error.details["confidence"], "confirmed");
    assert_eq!(api_error.details["remote_status"], 503);
    assert!(!api_error.details.contains_key("manual_retry_warning"));
}

#[test]
fn unclassified_raft_error_does_not_leak_the_source_message() {
    let api_error = super::super::map_raft_write_error(anyhow::anyhow!(
        "internal URL https://secret.example.invalid and token=secret"
    ));

    assert_eq!(api_error.code, "raft_write_outcome_unknown");
    assert_eq!(api_error.details["dispatch_state"], "unknown");
    assert!(!api_error.message.contains("secret"));
    assert!(!api_error.message.contains("https://"));
}
