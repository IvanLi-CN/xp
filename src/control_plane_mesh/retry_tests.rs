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
use tokio::{io::AsyncReadExt, net::TcpStream, sync::Mutex, task::JoinHandle};

#[derive(Clone)]
struct GatewayState {
    ca_key_pem: String,
    ca_cert_pem: String,
    remaining_failures: Arc<AtomicUsize>,
    request_count: Arc<AtomicUsize>,
}

async fn gateway(
    State(state): State<GatewayState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    state.request_count.fetch_add(1, Ordering::SeqCst);
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
        request_count: requests.clone(),
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

#[derive(Debug, Clone)]
struct AttemptObservation {
    request_id: String,
    issued_at: i64,
    idempotency_sha256: String,
}

#[derive(Clone)]
struct ObservationGatewayState {
    ca_key_pem: String,
    ca_cert_pem: String,
    observations: Arc<Mutex<Vec<AttemptObservation>>>,
    applied_idempotencies: Arc<Mutex<Vec<String>>>,
}

async fn observation_gateway(
    State(state): State<ObservationGatewayState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let verified = observe_request(&state, &method, &uri, &headers, &body).await;
    let acknowledgement = crate::internal_auth::sign_ack_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &verified,
        xp_test_fixtures::primary_node_id(),
        StatusCode::NO_CONTENT.as_u16(),
    )
    .expect("observation acknowledgement");
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(crate::internal_auth::INTERNAL_ACK_HEADER, acknowledgement)
        .body(axum::body::Body::empty())
        .expect("observation response")
}

async fn observe_request(
    state: &ObservationGatewayState,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> crate::internal_auth::VerifiedRequest {
    let verified = crate::internal_auth::verify_request_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        method,
        uri,
        headers,
        body,
        xp_test_fixtures::cluster_fixture53(),
        xp_test_fixtures::primary_node_id(),
    )
    .expect("observed dispatch receives a fresh valid signature");
    state.observations.lock().await.push(AttemptObservation {
        request_id: verified.context.request_id.clone(),
        issued_at: verified.context.issued_at,
        idempotency_sha256: verified.idempotency_sha256.clone(),
    });
    let mut applied = state.applied_idempotencies.lock().await;
    if !applied.contains(&verified.idempotency_sha256) {
        applied.push(verified.idempotency_sha256.clone());
    }
    verified
}

async fn read_raw_request(socket: &mut TcpStream) -> (Method, Uri, HeaderMap, Vec<u8>) {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = socket.read(&mut chunk).await.expect("read raw request");
        assert!(read > 0, "raw gateway request closed before headers");
        raw.extend_from_slice(&chunk[..read]);
        if let Some(position) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
    };
    let body_start = header_end + 4;
    let header_text = String::from_utf8(raw[..header_end].to_vec()).expect("raw request headers");
    let content_length = header_text
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().expect("content length"))
        })
        .unwrap_or_default();
    while raw.len() < body_start + content_length {
        let mut chunk = [0_u8; 4096];
        let read = socket
            .read(&mut chunk)
            .await
            .expect("read raw request body");
        assert!(read > 0, "raw gateway request closed before body");
        raw.extend_from_slice(&chunk[..read]);
    }
    let mut lines = header_text.lines();
    let mut request_line = lines.next().expect("raw request line").split_whitespace();
    let method = Method::from_bytes(request_line.next().expect("raw request method").as_bytes())
        .expect("raw request method parses");
    let uri = request_line
        .next()
        .expect("raw request URI")
        .parse()
        .expect("raw request URI parses");
    let mut headers = HeaderMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = axum::http::HeaderName::from_bytes(name.as_bytes()).expect("raw header name");
        let value = axum::http::HeaderValue::from_str(value.trim()).expect("raw header value");
        headers.insert(name, value);
    }
    (
        method,
        uri,
        headers,
        raw[body_start..body_start + content_length].to_vec(),
    )
}

async fn spawn_retry_observation_gateway(
    ca_key_pem: &str,
    ca_cert_pem: &str,
) -> (
    String,
    Arc<Mutex<Vec<AttemptObservation>>>,
    Arc<Mutex<Vec<String>>>,
    JoinHandle<()>,
) {
    let observations = Arc::new(Mutex::new(Vec::new()));
    let applied_idempotencies = Arc::new(Mutex::new(Vec::new()));
    let state = ObservationGatewayState {
        ca_key_pem: ca_key_pem.to_owned(),
        ca_cert_pem: ca_cert_pem.to_owned(),
        observations: observations.clone(),
        applied_idempotencies: applied_idempotencies.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("observation listener");
    let address = listener.local_addr().expect("observation address");
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("first retry connection");
        let (method, uri, headers, body) = read_raw_request(&mut socket).await;
        let _ = observe_request(&state, &method, &uri, &headers, &body).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        drop(socket);
        let _ = axum::serve(
            listener,
            Router::new()
                .fallback(any(observation_gateway))
                .with_state(state),
        )
        .await;
    });
    (
        format!("http://{address}"),
        observations,
        applied_idempotencies,
        task,
    )
}

#[test]
fn stale_pre_dispatch_context_fails_closed_without_panicking() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let request = MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/mesh/health".to_owned(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(1),
        allow_ambiguous_fallback: false,
        request_id: "stale-pre-dispatch-context".to_owned(),
        route: InternalRoute::HealthV2,
        cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
        sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
        updates_active_path: false,
    };
    let mut context = RequestContext::now(
        request.route,
        request.cluster_id.clone(),
        request.sender_id.clone(),
        xp_test_fixtures::primary_node_id(),
        request.request_id.clone(),
    );
    context.issued_at = context
        .issued_at
        .saturating_sub(crate::internal_auth::AUTH_WINDOW_SECS + 1);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        signed_headers(&request, &context, &ca.key_pem, &ca.cert_pem)
    }))
    .expect("stale local auth must not panic");
    assert!(matches!(
        result,
        Err(MeshRequestError::PreDispatchAuth(
            crate::internal_auth::AuthError::Invalid(message)
        )) if message == "request signature is outside the accepted clock window"
    ));
}

#[tokio::test]
async fn public_transport_retry_refreshes_timestamp_but_preserves_idempotency() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let (base_url, observations, applied_idempotencies, task) =
        spawn_retry_observation_gateway(&ca.key_pem, &ca.cert_pem).await;
    let peer = peer_target_tests::primary_reverse_target(None, base_url);
    let request = MeshRequest {
        method: reqwest::Method::POST,
        path_and_query: "/api/admin/_internal/raft/client-write".to_owned(),
        content_type: Some("application/json".to_owned()),
        body: br#"{"op":"set"}"#.to_vec(),
        total_budget: Duration::from_secs(4),
        allow_ambiguous_fallback: true,
        request_id: "fresh-dispatch-idempotency".to_owned(),
        route: InternalRoute::MeshV2,
        cluster_id: xp_test_fixtures::cluster_fixture53().to_owned(),
        sender_id: xp_test_fixtures::tertiary_node_id().to_owned(),
        updates_active_path: true,
    };
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    let result = client
        .send_peer_request(&peer, request, &ca.key_pem, &ca.cert_pem)
        .await;
    assert!(result.is_ok(), "transport retry should recover: {result:?}");

    let observations = observations.lock().await.clone();
    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].request_id, observations[1].request_id);
    assert_eq!(
        observations[0].idempotency_sha256,
        observations[1].idempotency_sha256
    );
    assert_ne!(observations[0].issued_at, observations[1].issued_at);
    assert_eq!(applied_idempotencies.lock().await.len(), 1);
    task.abort();
}

#[derive(Clone)]
struct IssuedAtRelayState {
    issued_at: Arc<Mutex<Vec<i64>>>,
    stall: bool,
}

async fn record_issued_at_relay(
    State(state): State<IssuedAtRelayState>,
    headers: HeaderMap,
) -> StatusCode {
    let issued_at = headers
        .get(crate::internal_auth::INTERNAL_ISSUED_AT_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("reverse relay receives an issued-at header")
        .parse::<i64>()
        .expect("reverse relay issued-at header parses");
    state.issued_at.lock().await.push(issued_at);
    if state.stall {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    StatusCode::SERVICE_UNAVAILABLE
}

async fn spawn_issued_at_relay(stall: bool) -> (String, Arc<Mutex<Vec<i64>>>, JoinHandle<()>) {
    let issued_at = Arc::new(Mutex::new(Vec::new()));
    let state = IssuedAtRelayState {
        issued_at: issued_at.clone(),
        stall,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("issued-at relay listener");
    let address = listener.local_addr().expect("issued-at relay address");
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            Router::new()
                .fallback(any(record_issued_at_relay))
                .with_state(state),
        )
        .await;
    });
    (format!("http://{address}"), issued_at, task)
}

#[tokio::test]
async fn reverse_public_fallback_refreshes_outer_signature_timestamp() {
    let (mesh_base_url, mesh_issued_at, mesh_task) = spawn_issued_at_relay(true).await;
    let (public_base_url, public_issued_at, public_task) = spawn_issued_at_relay(false).await;
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let rendezvous =
        peer_target_tests::secondary_reverse_target(Some(mesh_base_url), public_base_url);
    let peer = peer_target_tests::primary_reverse_target(None, "http://127.0.0.1:1".to_owned());
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    client
        .set_reverse_route(
            peer.node_id.clone(),
            peer_target_tests::reverse_route(
                rendezvous,
                None,
                peer_target_tests::reverse_assignment(),
            ),
        )
        .await;

    client
        .send_peer_reverse_request(
            &peer,
            peer_target_tests::reverse_request(),
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await
        .expect_err("relay responses omit signed acknowledgements");

    let mesh_issued_at = mesh_issued_at.lock().await.clone();
    let public_issued_at = public_issued_at.lock().await.clone();
    assert_eq!(mesh_issued_at.len(), 1);
    assert_eq!(public_issued_at.len(), 1);
    assert_ne!(mesh_issued_at[0], public_issued_at[0]);
    mesh_task.abort();
    public_task.abort();
}

#[tokio::test]
async fn invalid_reverse_target_is_not_reported_as_an_unknown_outcome() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let peer = peer_target_tests::primary_reverse_target(None, "http://127.0.0.1:1".to_owned());
    let rendezvous = peer_target_tests::secondary_reverse_target(
        Some("not-a-url".to_owned()),
        "https://public.example".to_owned(),
    );
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    client
        .set_reverse_route(
            peer.node_id.clone(),
            peer_target_tests::reverse_route(
                rendezvous,
                None,
                peer_target_tests::reverse_assignment(),
            ),
        )
        .await;

    let mut request = peer_target_tests::reverse_request();
    request.allow_ambiguous_fallback = false;
    let error = client
        .send_peer_request(&peer, request, &ca.key_pem, &ca.cert_pem)
        .await
        .expect_err("invalid reverse target must fail before dispatch");
    assert!(matches!(error, MeshRequestError::InvalidTarget(_)));
}

#[tokio::test]
async fn public_gateway_missing_ack_is_terminal_without_retry() {
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

    assert!(matches!(result, Err(MeshRequestError::Protocol(_))));
    assert_eq!(public_requests.load(Ordering::SeqCst), 1);
    public_task.abort();
}

#[tokio::test]
async fn public_transport_failure_retries_for_idempotent_request() {
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("public listener");
    let address = listener.local_addr().expect("public address");
    let requests = Arc::new(AtomicUsize::new(0));
    let state = GatewayState {
        ca_key_pem: ca.key_pem.clone(),
        ca_cert_pem: ca.cert_pem.clone(),
        remaining_failures: Arc::new(AtomicUsize::new(0)),
        request_count: requests.clone(),
    };
    let task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("first public connection");
        drop(socket);
        let _ = axum::serve(
            listener,
            Router::new().fallback(any(gateway)).with_state(state),
        )
        .await;
    });
    let peer = peer_target_tests::primary_reverse_target(None, format!("http://{address}"));
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
                request_id: "public-transport-retry".to_string(),
                route: InternalRoute::HealthV2,
                cluster_id: xp_test_fixtures::cluster_fixture53().to_string(),
                sender_id: xp_test_fixtures::tertiary_node_id().to_string(),
                updates_active_path: true,
            },
            &ca.key_pem,
            &ca.cert_pem,
        )
        .await;

    assert!(result.is_ok(), "transport retry should recover: {result:?}");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn confirmed_timeout_survives_a_later_public_transport_error() {
    let error = reqwest::Client::new()
        .get("http://127.0.0.1:1")
        .send()
        .await
        .expect_err("the test port must not be listening");

    assert!(error.is_connect());
    assert!(matches!(
        retry::classify_public_retry_failure(error, true, true),
        MeshRequestError::TransportTimeout
    ));
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
        cluster_id: xp_test_fixtures::cluster_fixture53().to_string(),
        sender_id: "sender".to_string(),
        updates_active_path: true,
    };
    assert!(!retry::request_allows_public_gateway_retry(&request));
    request.path_and_query = "/raft/append".to_string();
    assert!(retry::request_allows_public_gateway_retry(&request));
    request.path_and_query = "/api/admin/_internal/raft/client-write".to_string();
    assert!(retry::request_allows_public_gateway_retry(&request));
}

#[test]
fn read_retry_does_not_require_ambiguous_fallback() {
    let request = MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: "/api/admin/_internal/capabilities".to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(1),
        allow_ambiguous_fallback: false,
        request_id: "read-retry-policy".to_string(),
        route: InternalRoute::MeshV2,
        cluster_id: xp_test_fixtures::cluster_fixture53().to_string(),
        sender_id: "sender".to_string(),
        updates_active_path: false,
    };
    assert!(retry::request_allows_public_gateway_retry(&request));
}

#[tokio::test]
async fn reverse_control_does_not_replay_unknown_result_on_standby() {
    let (primary_base_url, primary_requests, primary_task) =
        peer_target_tests::spawn_reverse_relay_counter().await;
    let (standby_base_url, standby_requests, standby_task) =
        peer_target_tests::spawn_reverse_relay_counter().await;
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::cluster_fixture53())
        .expect("cluster CA");
    let mut assignment = peer_target_tests::reverse_assignment();
    assignment.standby_node_id = Some(xp_test_fixtures::tertiary_node_id().to_owned());
    let rendezvous = peer_target_tests::secondary_reverse_target(None, primary_base_url);
    let standby = peer_target_tests::tertiary_reverse_target(None, standby_base_url);
    let peer = peer_target_tests::primary_reverse_target(None, "http://127.0.0.1:1".to_string());
    let client = MeshAwareHttpClient::new(reqwest::Client::new());
    client
        .set_reverse_route(
            peer.node_id.clone(),
            peer_target_tests::reverse_route(rendezvous, Some(standby), assignment),
        )
        .await;
    let mut request = peer_target_tests::reverse_request();
    request.allow_ambiguous_fallback = false;
    let error = client
        .send_peer_reverse_request(&peer, request, &ca.key_pem, &ca.cert_pem)
        .await
        .expect_err("missing relay acknowledgement must be outcome-unknown");
    assert!(matches!(error, MeshRequestError::OutcomeUnknown));
    assert_eq!(primary_requests.load(Ordering::SeqCst), 1);
    assert_eq!(standby_requests.load(Ordering::SeqCst), 0);
    primary_task.abort();
    standby_task.abort();
}
