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
