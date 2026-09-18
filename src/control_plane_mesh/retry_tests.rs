use super::*;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
    routing::any,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::task::JoinHandle;

#[derive(Clone)]
struct GatewayState {
    ca_key_pem: String,
    ca_cert_pem: String,
    remaining_failures: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
}

async fn gateway(
    State(state): State<GatewayState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    if state
        .remaining_failures
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
            remaining.checked_sub(1)
        })
        .is_ok()
    {
        return Response::builder()
            .status(StatusCode::GATEWAY_TIMEOUT)
            .body(axum::body::Body::empty())
            .expect("gateway response");
    }
    let verified = crate::internal_auth::verify_request_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &method,
        &uri,
        &headers,
        &body,
        xp_test_fixtures::cluster_fixture53(),
        xp_test_fixtures::primary_node_id(),
    )
    .expect("public fallback receives a signed request");
    let ack = crate::internal_auth::sign_ack_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &verified,
        xp_test_fixtures::primary_node_id(),
        StatusCode::OK.as_u16(),
    )
    .expect("sign public acknowledgement");
    Response::builder()
        .status(StatusCode::OK)
        .header(crate::internal_auth::INTERNAL_ACK_HEADER, ack)
        .body(axum::body::Body::empty())
        .expect("public fallback response")
}

async fn spawn_gateway(
    ca_key_pem: &str,
    ca_cert_pem: &str,
    failures: usize,
) -> (String, Arc<AtomicUsize>, JoinHandle<()>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let state = GatewayState {
        ca_key_pem: ca_key_pem.to_owned(),
        ca_cert_pem: ca_cert_pem.to_owned(),
        remaining_failures: Arc::new(AtomicUsize::new(failures)),
        requests: requests.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("public listener");
    let address = listener.local_addr().expect("public address");
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            Router::new().fallback(any(gateway)).with_state(state),
        )
        .await;
    });
    (format!("http://{address}"), requests, task)
}
#[tokio::test]
async fn public_gateway_timeout_retries_before_reporting_unknown() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (public_base_url, public_requests, public_task) =
        spawn_gateway(&ca.key_pem, &ca.cert_pem, 1).await;
    let peer = peer_target_tests::primary_reverse_target(None, public_base_url);
    let client =
        MeshAwareHttpClient::from_transport_clients(reqwest::Client::new(), reqwest::Client::new());

    let result = client
        .send_peer_request(
            &peer,
            MeshRequest {
                method: reqwest::Method::GET,
                path_and_query: "/api/admin/_internal/mesh/health".to_string(),
                content_type: None,
                body: Vec::new(),
                total_budget: Duration::from_secs(2),
                allow_ambiguous_fallback: true,
                request_id: "public-gateway-retry".to_string(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_string(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_string(),
                updates_active_path: true,
            },
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await;

    assert!(result.is_ok(), "gateway retry should recover: {result:?}");
    assert_eq!(public_requests.load(Ordering::SeqCst), 2);
    public_task.abort();
}

#[test]
fn public_gateway_retry_excludes_non_idempotent_mutations() {
    let mut request = MeshRequest {
        method: reqwest::Method::POST,
        path_and_query: "/api/admin/_internal/endpoint-probe/run".to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(1),
        allow_ambiguous_fallback: true,
        request_id: "retry-policy".to_string(),
        route: InternalRoute::MeshV2,
        cluster_id: "cluster".to_string(),
        sender_id: "sender".to_string(),
        updates_active_path: true,
    };
    assert!(!retry::request_allows_public_gateway_retry(&request));
    request.path_and_query = "/raft/append".to_string();
    assert!(retry::request_allows_public_gateway_retry(&request));
    request.path_and_query = "/api/admin/_internal/raft/client-write".to_string();
    assert!(retry::request_allows_public_gateway_retry(&request));
}
