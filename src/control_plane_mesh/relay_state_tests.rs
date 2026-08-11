use super::*;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    routing::post,
};
use tokio::net::TcpListener;

#[derive(Clone)]
enum AckResponse {
    Missing,
    Invalid,
}

async fn respond_with_ack(State(response): State<AckResponse>) -> (StatusCode, HeaderMap) {
    let mut headers = HeaderMap::new();
    if matches!(response, AckResponse::Invalid) {
        headers.insert(
            internal_auth::INTERNAL_ACK_HEADER,
            HeaderValue::from_static("v2:not-a-valid-acknowledgement"),
        );
    }
    (StatusCode::OK, headers)
}

async fn response_server(response: AckResponse) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/internal", post(respond_with_ack))
        .with_state(response);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{address}/internal")
}

async fn closed_proxy_client() -> reqwest::Client {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{address}")).unwrap())
        .build()
        .unwrap()
}

fn request(allow_ambiguous_fallback: bool) -> MeshRequest {
    MeshRequest {
        method: reqwest::Method::POST,
        path_and_query: "/internal".to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(1),
        allow_ambiguous_fallback,
        request_id: "request-1".to_string(),
        route: InternalRoute::MeshV2,
        cluster_id: xp_test_fixtures::cluster_fixture511().to_owned(),
        sender_id: "node-a".to_string(),
        updates_active_path: false,
    }
}

fn context() -> RequestContext {
    RequestContext::now(
        InternalRoute::MeshV2,
        "cluster-1",
        "node-a",
        "peer-a",
        "request-1",
    )
}

fn ca() -> crate::cluster_identity::ClusterCaPem {
    crate::cluster_identity::generate_cluster_ca("01JTESTCLUSTERID00000000000000").unwrap()
}

#[tokio::test]
async fn relay_ack_failures_mark_control_relay_degraded() {
    let state = MeshProxyStateHandle::ready();
    let client = MeshAwareHttpClient::new(
        reqwest::Client::new(),
        Some(reqwest::Client::new()),
        state.clone(),
    );
    let ca = ca();

    let error = client
        .send_public_signed(
            &response_server(AckResponse::Missing).await,
            &request(true),
            &context(),
            &ca.key_pem,
            &ca.cert_pem,
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, MeshRequestError::Protocol(_)));
    let snapshot = state.snapshot().await;
    assert_eq!(snapshot.status, MeshProxyStatus::Degraded);
    assert!(
        snapshot
            .fallback_reason
            .as_deref()
            .unwrap()
            .contains("control relay response has no signed acknowledgement")
    );
}

#[tokio::test]
async fn relay_ack_verification_failures_mark_control_relay_degraded() {
    let state = MeshProxyStateHandle::ready();
    let client = MeshAwareHttpClient::new(
        reqwest::Client::new(),
        Some(reqwest::Client::new()),
        state.clone(),
    );
    let ca = ca();

    let error = client
        .send_public_signed(
            &response_server(AckResponse::Invalid).await,
            &request(true),
            &context(),
            &ca.key_pem,
            &ca.cert_pem,
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, MeshRequestError::Auth(_)));
    assert_eq!(state.snapshot().await.status, MeshProxyStatus::Degraded);
}

#[tokio::test]
async fn failed_relay_then_direct_ack_failure_marks_control_relay_degraded() {
    let state = MeshProxyStateHandle::ready();
    let client = MeshAwareHttpClient::new(
        reqwest::Client::new(),
        Some(closed_proxy_client().await),
        state.clone(),
    );
    let ca = ca();

    let error = client
        .send_public_signed(
            &response_server(AckResponse::Missing).await,
            &request(true),
            &context(),
            &ca.key_pem,
            &ca.cert_pem,
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, MeshRequestError::Protocol(_)));
    let snapshot = state.snapshot().await;
    assert_eq!(snapshot.status, MeshProxyStatus::Degraded);
    assert!(
        snapshot
            .fallback_reason
            .as_deref()
            .unwrap()
            .contains("control relay fallback response")
    );
}

#[tokio::test]
async fn unsafe_relay_failure_never_falls_back_to_direct() {
    let state = MeshProxyStateHandle::ready();
    let client = MeshAwareHttpClient::new(
        reqwest::Client::new(),
        Some(closed_proxy_client().await),
        state.clone(),
    );
    let ca = ca();

    let error = client
        .send_public_signed(
            "http://127.0.0.1:1/internal",
            &request(false),
            &context(),
            &ca.key_pem,
            &ca.cert_pem,
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, MeshRequestError::OutcomeUnknown));
    assert_eq!(state.snapshot().await.status, MeshProxyStatus::Degraded);
}
