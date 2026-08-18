use super::*;

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri, Version},
    response::Response,
    routing::any,
};
use futures_util::{future::join_all, stream};
use rcgen::{CertificateParams, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{io::copy_bidirectional, net::TcpListener, task::JoinHandle};
use tokio::{sync::broadcast, time::sleep};

const HEALTH_PATH: &str = "/api/admin/_internal/mesh/health";

#[derive(Clone)]
struct SignedServerState {
    ca_key_pem: String,
    ca_cert_pem: String,
}

struct CountingTlsServer {
    addr: std::net::SocketAddr,
    accepts: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    peak_active: Arc<AtomicUsize>,
    disconnect: broadcast::Sender<()>,
    proxy_task: JoinHandle<()>,
    server_task: JoinHandle<()>,
}

struct Http1Server {
    addr: std::net::SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for Http1Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Drop for CountingTlsServer {
    fn drop(&mut self) {
        self.proxy_task.abort();
        self.server_task.abort();
    }
}

impl CountingTlsServer {
    async fn disconnect_all(&self) {
        let _ = self.disconnect.send(());
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while self.active.load(Ordering::SeqCst) != 0 {
                sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("connections close");
    }
}

async fn signed_health(
    State(state): State<SignedServerState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<axum::body::Body> {
    let verified = crate::internal_auth::verify_request_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &method,
        &uri,
        &headers,
        &body,
        xp_test_fixtures::primary_cluster_id(),
        xp_test_fixtures::primary_node_id(),
    )
    .expect("valid signed request");
    let ack = if uri.query() == Some("invalid-ack") {
        "v2:not-a-valid-acknowledgement".to_string()
    } else {
        crate::internal_auth::sign_ack_v2(
            &state.ca_key_pem,
            &state.ca_cert_pem,
            &verified,
            xp_test_fixtures::primary_node_id(),
            StatusCode::OK.as_u16(),
        )
        .expect("sign acknowledgement")
    };
    let response_body = if uri.query() == Some("stream") {
        axum::body::Body::from_stream(stream::pending::<Result<Bytes, std::io::Error>>())
    } else {
        axum::body::Body::empty()
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(crate::internal_auth::INTERNAL_ACK_HEADER, ack)
        .body(response_body)
        .expect("response")
}

fn signed_app(ca_key_pem: &str, ca_cert_pem: &str) -> Router {
    Router::new()
        .fallback(any(signed_health))
        .layer(axum::extract::DefaultBodyLimit::disable())
        .with_state(SignedServerState {
            ca_key_pem: ca_key_pem.to_string(),
            ca_cert_pem: ca_cert_pem.to_string(),
        })
}

async fn spawn_counting_tls_server(ca_key_pem: &str, ca_cert_pem: &str) -> CountingTlsServer {
    let ca_key = KeyPair::from_pem(ca_key_pem).expect("CA key");
    let ca_cert = Issuer::from_ca_cert_pem(ca_cert_pem, ca_key).expect("CA certificate");
    let cert_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("server key");
    let cert = CertificateParams::new(vec!["127.0.0.1".to_string()])
        .expect("certificate params")
        .signed_by(&cert_key, &ca_cert)
        .expect("server certificate");
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert.pem().into_bytes(),
        cert_key.serialize_pem().into_bytes(),
    )
    .await
    .expect("TLS config");
    let server_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server listener");
    server_listener
        .set_nonblocking(true)
        .expect("nonblocking server listener");
    let server_addr = server_listener.local_addr().expect("server address");
    let app = signed_app(ca_key_pem, ca_cert_pem);
    let server = axum_server::from_tcp_rustls(server_listener, tls)
        .expect("TLS server")
        .serve(app.into_make_service());
    let server_task = tokio::spawn(async move {
        let _ = server.into_future().await;
    });

    let proxy_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("proxy listener");
    let addr = proxy_listener.local_addr().expect("proxy address");
    let accepts = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let peak_active = Arc::new(AtomicUsize::new(0));
    let (disconnect, _) = broadcast::channel(1);
    let accepts_for_task = accepts.clone();
    let active_for_task = active.clone();
    let peak_for_task = peak_active.clone();
    let disconnect_for_task = disconnect.clone();
    let proxy_task = tokio::spawn(async move {
        loop {
            let Ok((mut downstream, _)) = proxy_listener.accept().await else {
                break;
            };
            accepts_for_task.fetch_add(1, Ordering::SeqCst);
            let active = active_for_task.clone();
            let peak_active = peak_for_task.clone();
            let mut disconnect = disconnect_for_task.subscribe();
            tokio::spawn(async move {
                let Ok(mut upstream) = tokio::net::TcpStream::connect(server_addr).await else {
                    return;
                };
                let active_now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak_active.fetch_max(active_now, Ordering::SeqCst);
                tokio::select! {
                    _ = copy_bidirectional(&mut downstream, &mut upstream) => {}
                    _ = disconnect.recv() => {}
                }
                active.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    CountingTlsServer {
        addr,
        accepts,
        active,
        peak_active,
        disconnect,
        proxy_task,
        server_task,
    }
}

async fn spawn_http1_server(ca_key_pem: &str, ca_cert_pem: &str) -> Http1Server {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("HTTP/1 listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking HTTP/1 listener");
    let addr = listener.local_addr().expect("HTTP/1 address");
    let server = axum_server::from_tcp(listener)
        .expect("HTTP/1 server")
        .http1_only()
        .serve(signed_app(ca_key_pem, ca_cert_pem).into_make_service());
    let task = tokio::spawn(async move {
        let _ = server.into_future().await;
    });
    Http1Server { addr, task }
}

fn mesh_request(index: usize) -> MeshRequest {
    MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: HEALTH_PATH.to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: std::time::Duration::from_secs(5),
        allow_ambiguous_fallback: true,
        request_id: format!("request-{index}"),
        route: InternalRoute::HealthV2,
        cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
        sender_id: xp_test_fixtures::secondary_node_id().to_owned(),
        updates_active_path: true,
    }
}

fn mesh_target(addr: std::net::SocketAddr) -> MeshPeerTarget {
    MeshPeerTarget {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        mesh_base_url: Some(format!("https://{addr}")),
        mesh_reason: crate::mesh_telemetry::MeshPeerReason::MeshAvailable,
        public_base_url: xp_test_fixtures::public_fallback_url().to_owned(),
    }
}

#[tokio::test]
async fn mesh_requests_reuse_one_tls_connection_for_sequential_and_parallel_load() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::primary_cluster_id())
        .expect("cluster CA");
    let csr = crate::cluster_identity::generate_node_keypair_and_csr(
        xp_test_fixtures::secondary_node_id(),
    )
    .expect("node CSR");
    let node_cert = crate::cluster_identity::sign_node_csr(
        xp_test_fixtures::primary_cluster_id(),
        &ca.key_pem,
        &csr.csr_pem,
    )
    .expect("node certificate");
    let server = spawn_counting_tls_server(&ca.key_pem, &ca.cert_pem).await;
    let telemetry_dir = tempfile::tempdir().expect("telemetry directory");
    let telemetry = crate::mesh_telemetry::MeshTelemetryHandle::load(telemetry_dir.path())
        .expect("Mesh telemetry");
    let factory = HttpNetworkFactory::try_new_mtls(&ca.cert_pem, &node_cert, &csr.key_pem)
        .expect("network factory");
    let client = factory
        .mesh_client()
        .with_mesh_observability(telemetry.clone());
    let target = mesh_target(server.addr);

    for index in 0..32 {
        let response = client
            .send_peer_request(&target, mesh_request(index), &ca.key_pem, &ca.cert_pem)
            .await
            .expect("sequential Mesh request");
        assert_eq!(response.version(), Version::HTTP_2);
    }

    let responses = join_all((32..48).map(|index| {
        let client = client.clone();
        let target = target.clone();
        let ca_key_pem = ca.key_pem.clone();
        let ca_cert_pem = ca.cert_pem.clone();
        async move {
            client
                .send_peer_request(&target, mesh_request(index), &ca_key_pem, &ca_cert_pem)
                .await
        }
    }))
    .await;
    for response in responses {
        assert_eq!(
            response.expect("parallel Mesh request").version(),
            Version::HTTP_2
        );
    }

    assert_eq!(server.accepts.load(Ordering::SeqCst), 1);
    let peer = telemetry.snapshot().await.peers.remove(0);
    assert_eq!(peer.connection_generation, 1);
    assert_eq!(peer.current_connection_requests, 48);
    assert_eq!(
        peer.buckets
            .iter()
            .map(|bucket| bucket.mesh_h2_requests)
            .sum::<u32>(),
        48
    );
    assert_eq!(
        peer.buckets
            .iter()
            .map(|bucket| bucket.mesh_connection_starts)
            .sum::<u32>(),
        1
    );
}

#[tokio::test]
async fn mesh_request_reconnects_once_after_the_active_connection_is_cut() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::primary_cluster_id())
        .expect("cluster CA");
    let csr = crate::cluster_identity::generate_node_keypair_and_csr(
        xp_test_fixtures::secondary_node_id(),
    )
    .expect("node CSR");
    let node_cert = crate::cluster_identity::sign_node_csr(
        xp_test_fixtures::primary_cluster_id(),
        &ca.key_pem,
        &csr.csr_pem,
    )
    .expect("node certificate");
    let server = spawn_counting_tls_server(&ca.key_pem, &ca.cert_pem).await;
    let telemetry_dir = tempfile::tempdir().expect("telemetry directory");
    let telemetry = crate::mesh_telemetry::MeshTelemetryHandle::load(telemetry_dir.path())
        .expect("Mesh telemetry");
    let client = HttpNetworkFactory::try_new_mtls(&ca.cert_pem, &node_cert, &csr.key_pem)
        .expect("network factory")
        .mesh_client()
        .with_mesh_observability(telemetry.clone());
    let target = mesh_target(server.addr);

    let first = client
        .send_peer_request(&target, mesh_request(0), &ca.key_pem, &ca.cert_pem)
        .await
        .expect("first Mesh request");
    assert_eq!(first.version(), Version::HTTP_2);
    drop(first);
    server.disconnect_all().await;

    let second = client
        .send_peer_request(&target, mesh_request(1), &ca.key_pem, &ca.cert_pem)
        .await
        .expect("Mesh reconnect");
    assert_eq!(second.version(), Version::HTTP_2);
    sleep(std::time::Duration::from_millis(20)).await;

    assert_eq!(server.accepts.load(Ordering::SeqCst), 2);
    assert!(server.peak_active.load(Ordering::SeqCst) <= 2);
    assert_eq!(server.active.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn mesh_pool_discards_idle_connections_after_the_policy_timeout() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::primary_cluster_id())
        .expect("cluster CA");
    let csr = crate::cluster_identity::generate_node_keypair_and_csr(
        xp_test_fixtures::secondary_node_id(),
    )
    .expect("node CSR");
    let node_cert = crate::cluster_identity::sign_node_csr(
        xp_test_fixtures::primary_cluster_id(),
        &ca.key_pem,
        &csr.csr_pem,
    )
    .expect("node certificate");
    let server = spawn_counting_tls_server(&ca.key_pem, &ca.cert_pem).await;
    let client = crate::control_plane_mesh::build_mesh_http_client_with_policy(
        &ca.cert_pem,
        &node_cert,
        &csr.key_pem,
        crate::control_plane_mesh::MeshTransportPolicy {
            pool_idle_timeout: std::time::Duration::from_millis(50),
        },
    )
    .expect("Mesh client");
    let target = mesh_target(server.addr);

    let first = client
        .send_peer_request(&target, mesh_request(0), &ca.key_pem, &ca.cert_pem)
        .await
        .expect("first Mesh request");
    drop(first);
    sleep(std::time::Duration::from_millis(150)).await;
    let second = client
        .send_peer_request(&target, mesh_request(1), &ca.key_pem, &ca.cert_pem)
        .await
        .expect("Mesh request after idle timeout");
    assert_eq!(second.version(), Version::HTTP_2);

    assert_eq!(server.accepts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn h2_transport_failure_uses_the_compatible_public_client() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::primary_cluster_id())
        .expect("cluster CA");
    let csr = crate::cluster_identity::generate_node_keypair_and_csr(
        xp_test_fixtures::secondary_node_id(),
    )
    .expect("node CSR");
    let node_cert = crate::cluster_identity::sign_node_csr(
        xp_test_fixtures::primary_cluster_id(),
        &ca.key_pem,
        &csr.csr_pem,
    )
    .expect("node certificate");
    let server = spawn_http1_server(&ca.key_pem, &ca.cert_pem).await;
    let telemetry_dir = tempfile::tempdir().expect("telemetry directory");
    let telemetry = crate::mesh_telemetry::MeshTelemetryHandle::load(telemetry_dir.path())
        .expect("Mesh telemetry");
    let client = HttpNetworkFactory::try_new_mtls(&ca.cert_pem, &node_cert, &csr.key_pem)
        .expect("network factory")
        .mesh_client()
        .with_mesh_observability(telemetry.clone());
    let target = MeshPeerTarget {
        public_base_url: format!("http://{}", server.addr),
        ..mesh_target(server.addr)
    };

    let response = client
        .send_peer_request(&target, mesh_request(0), &ca.key_pem, &ca.cert_pem)
        .await
        .expect("public fallback response");
    assert_eq!(response.version(), Version::HTTP_11);

    let peer = telemetry.snapshot().await.peers.remove(0);
    let bucket = peer.buckets.back().expect("telemetry bucket");
    assert_eq!(bucket.mesh_failure, 1);
    assert_eq!(bucket.public_success, 1);
    assert_eq!(bucket.fallback_success, 1);
    assert_eq!(peer.connection_generation, 0);
}

#[tokio::test]
async fn invalid_h2_ack_never_downgrades_to_public_transport() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::primary_cluster_id())
        .expect("cluster CA");
    let csr = crate::cluster_identity::generate_node_keypair_and_csr(
        xp_test_fixtures::secondary_node_id(),
    )
    .expect("node CSR");
    let node_cert = crate::cluster_identity::sign_node_csr(
        xp_test_fixtures::primary_cluster_id(),
        &ca.key_pem,
        &csr.csr_pem,
    )
    .expect("node certificate");
    let server = spawn_counting_tls_server(&ca.key_pem, &ca.cert_pem).await;
    let client = HttpNetworkFactory::try_new_mtls(&ca.cert_pem, &node_cert, &csr.key_pem)
        .expect("network factory")
        .mesh_client();
    let target = mesh_target(server.addr);
    let mut request = mesh_request(0);
    request.path_and_query = format!("{HEALTH_PATH}?invalid-ack");

    let error = client
        .send_peer_request(&target, request, &ca.key_pem, &ca.cert_pem)
        .await
        .expect_err("invalid Mesh acknowledgement");
    assert!(matches!(
        error,
        crate::control_plane_mesh::MeshRequestError::Auth(_)
    ));
}

#[tokio::test]
async fn long_lived_stream_and_large_request_share_one_h2_connection() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca = crate::cluster_identity::generate_cluster_ca(xp_test_fixtures::primary_cluster_id())
        .expect("cluster CA");
    let csr = crate::cluster_identity::generate_node_keypair_and_csr(
        xp_test_fixtures::secondary_node_id(),
    )
    .expect("node CSR");
    let node_cert = crate::cluster_identity::sign_node_csr(
        xp_test_fixtures::primary_cluster_id(),
        &ca.key_pem,
        &csr.csr_pem,
    )
    .expect("node certificate");
    let server = spawn_counting_tls_server(&ca.key_pem, &ca.cert_pem).await;
    let telemetry_dir = tempfile::tempdir().expect("telemetry directory");
    let telemetry = crate::mesh_telemetry::MeshTelemetryHandle::load(telemetry_dir.path())
        .expect("Mesh telemetry");
    let client = HttpNetworkFactory::try_new_mtls(&ca.cert_pem, &node_cert, &csr.key_pem)
        .expect("network factory")
        .mesh_client()
        .with_mesh_observability(telemetry.clone());
    let target = mesh_target(server.addr);

    let mut stream_request = mesh_request(0);
    stream_request.route = InternalRoute::MeshV2;
    stream_request.path_and_query =
        "/api/admin/_internal/nodes/runtime/local/events?stream".to_string();
    let stream_response = client
        .send_peer_request(&target, stream_request, &ca.key_pem, &ca.cert_pem)
        .await
        .expect("long-lived Mesh response");

    let mut requests = (1..=8)
        .map(|index| {
            let mut request = mesh_request(index);
            request.route = InternalRoute::MeshV2;
            request.method = reqwest::Method::POST;
            request.path_and_query = "/raft/append".to_string();
            request.content_type = Some("application/json".to_string());
            request.body = br#"{"entries":[]}"#.to_vec();
            request
        })
        .collect::<Vec<_>>();
    let mut snapshot = mesh_request(9);
    snapshot.route = InternalRoute::MeshV2;
    snapshot.method = reqwest::Method::POST;
    snapshot.path_and_query = "/raft/snapshot".to_string();
    snapshot.content_type = Some("application/octet-stream".to_string());
    snapshot.body = vec![0x5a; 8 * 1024 * 1024];
    requests.push(snapshot);
    let mut fanout = mesh_request(10);
    fanout.route = InternalRoute::MeshV2;
    fanout.path_and_query = "/api/admin/_internal/nodes/runtime/local".to_string();
    requests.push(fanout);
    for request in &mut requests {
        request.total_budget = std::time::Duration::from_secs(30);
    }

    let responses = join_all(requests.into_iter().map(|request| {
        let request_path = request.path_and_query.clone();
        let client = client.clone();
        let target = target.clone();
        let ca_key_pem = ca.key_pem.clone();
        let ca_cert_pem = ca.cert_pem.clone();
        async move {
            (
                request_path,
                client
                    .send_peer_request(&target, request, &ca_key_pem, &ca_cert_pem)
                    .await,
            )
        }
    }))
    .await;

    assert_eq!(stream_response.version(), Version::HTTP_2);
    let failures = responses
        .iter()
        .filter_map(|(request_path, response)| {
            response
                .as_ref()
                .err()
                .map(|error| format!("{request_path}: {error:?}"))
        })
        .collect::<Vec<_>>();
    if !failures.is_empty() {
        panic!(
            "multiplexed Mesh failures: {failures:?}; telemetry: {:?}",
            telemetry.snapshot().await.events
        );
    }
    for (request_path, response) in responses {
        assert_eq!(
            response
                .unwrap_or_else(|error| panic!(
                    "multiplexed Mesh response for {request_path}: {error:?}"
                ))
                .version(),
            Version::HTTP_2
        );
    }
    assert_eq!(server.accepts.load(Ordering::SeqCst), 1);
}
