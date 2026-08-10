use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri, Version},
    response::Response,
    routing::any,
};
use futures_util::future::join_all;
use rand::rngs::OsRng;
use rcgen::{CertificateParams, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256};
use tokio::{
    io::copy_bidirectional,
    net::TcpListener,
    sync::broadcast,
    task::JoinHandle,
    time::{Instant, sleep},
};

use xp::{
    control_plane_mesh::{
        MeshPeerTarget, MeshProxyStateHandle, MeshRequest, build_mesh_http_client,
    },
    domain::{Endpoint, EndpointKind},
    internal_auth::{self, InternalRoute},
    mesh_telemetry::MeshPeerReason,
    protocol::{
        MihomoSmuxConfig, RealityConfig, RealityKeys, RealityServerNamesSource,
        VlessRealityVisionTcpEndpointMeta, generate_reality_keypair,
    },
    xray,
};

const HEALTH_PATH: &str = "/api/admin/_internal/mesh/health";

#[derive(Clone)]
struct SignedServerState {
    ca_key_pem: String,
    ca_cert_pem: String,
}

struct TestServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct CountingProxy {
    addr: SocketAddr,
    accepts: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    peak_active: Arc<AtomicUsize>,
    disconnect: broadcast::Sender<()>,
    task: JoinHandle<()>,
}

impl Drop for CountingProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl CountingProxy {
    async fn disconnect_all(&self) {
        let _ = self.disconnect.send(());
        tokio::time::timeout(Duration::from_secs(1), async {
            while self.active.load(Ordering::SeqCst) != 0 {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("proxied connections close");
    }
}

async fn signed_health(
    State(state): State<SignedServerState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<axum::body::Body> {
    let verified = internal_auth::verify_request_v2(
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
    let ack = internal_auth::sign_ack_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &verified,
        xp_test_fixtures::primary_node_id(),
        StatusCode::OK.as_u16(),
    )
    .expect("sign acknowledgement");
    Response::builder()
        .status(StatusCode::OK)
        .header(internal_auth::INTERNAL_ACK_HEADER, ack)
        .body(axum::body::Body::empty())
        .expect("response")
}

async fn spawn_signed_tls_server(ca_key_pem: &str, ca_cert_pem: &str) -> TestServer {
    let ca_key = KeyPair::from_pem(ca_key_pem).expect("CA key");
    let ca_cert = Issuer::from_ca_cert_pem(ca_cert_pem, ca_key).expect("CA certificate");
    let cert_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("server key");
    let cert = CertificateParams::new(xp_test_fixtures::loopback_server_names())
        .expect("certificate params")
        .signed_by(&cert_key, &ca_cert)
        .expect("server certificate");
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert.pem().into_bytes(),
        cert_key.serialize_pem().into_bytes(),
    )
    .await
    .expect("TLS config");
    let listener = std::net::TcpListener::bind("0.0.0.0:0").expect("server listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking server listener");
    let addr = listener.local_addr().expect("server address");
    let app = Router::new()
        .route(HEALTH_PATH, any(signed_health))
        .with_state(SignedServerState {
            ca_key_pem: ca_key_pem.to_string(),
            ca_cert_pem: ca_cert_pem.to_string(),
        });
    let server = axum_server::from_tcp_rustls(listener, tls)
        .expect("TLS server")
        .serve(app.into_make_service());
    let task = tokio::spawn(async move {
        let _ = server.into_future().await;
    });
    TestServer { addr, task }
}

async fn spawn_counting_proxy(upstream: SocketAddr) -> CountingProxy {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("counting proxy listener");
    let addr = listener.local_addr().expect("counting proxy address");
    let accepts = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let peak_active = Arc::new(AtomicUsize::new(0));
    let (disconnect, _) = broadcast::channel(1);
    let accepts_for_task = accepts.clone();
    let active_for_task = active.clone();
    let peak_for_task = peak_active.clone();
    let disconnect_for_task = disconnect.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut downstream, _)) = listener.accept().await else {
                break;
            };
            accepts_for_task.fetch_add(1, Ordering::SeqCst);
            let active = active_for_task.clone();
            let peak_active = peak_for_task.clone();
            let mut disconnect = disconnect_for_task.subscribe();
            tokio::spawn(async move {
                let Ok(mut upstream) = tokio::net::TcpStream::connect(upstream).await else {
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
    CountingProxy {
        addr,
        accepts,
        active,
        peak_active,
        disconnect,
        task,
    }
}

fn mesh_request(index: usize) -> MeshRequest {
    MeshRequest {
        method: reqwest::Method::GET,
        path_and_query: HEALTH_PATH.to_string(),
        content_type: None,
        body: Vec::new(),
        total_budget: Duration::from_secs(5),
        allow_ambiguous_fallback: true,
        request_id: format!("xray-mesh-{index}"),
        route: InternalRoute::HealthV2,
        cluster_id: xp_test_fixtures::primary_cluster_id().to_owned(),
        sender_id: xp_test_fixtures::secondary_node_id().to_owned(),
        updates_active_path: true,
    }
}

fn mesh_target(proxy: &CountingProxy) -> MeshPeerTarget {
    MeshPeerTarget {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        mesh_base_url: Some(format!(
            "https://{}:{}",
            xp_test_fixtures::loopback_address(),
            proxy.addr.port()
        )),
        mesh_reason: MeshPeerReason::MeshAvailable,
        public_base_url: xp_test_fixtures::public_fallback_url().to_owned(),
    }
}

async fn wait_for_inbound(addr: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(_) => return,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                sleep(Duration::from_millis(50)).await;
            }
            Err(error) => panic!("Xray Reality inbound did not become ready: {error}"),
        }
    }
}

#[tokio::test]
#[ignore]
async fn reality_fallback_reuses_one_h2_connection_and_recovers_after_disconnect() {
    if std::env::var("XP_E2E_XRAY_MODE").ok().as_deref() != Some("external") {
        return;
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let xray_api_addr: SocketAddr = std::env::var("XP_E2E_XRAY_API_ADDR")
        .expect("XP_E2E_XRAY_API_ADDR")
        .parse()
        .expect("valid Xray API address");
    let vless_port: u16 = std::env::var("XP_E2E_VLESS_PORT")
        .expect("XP_E2E_VLESS_PORT")
        .parse()
        .expect("valid VLESS port");

    let ca = xp::cluster_identity::generate_cluster_ca(xp_test_fixtures::primary_cluster_id())
        .expect("cluster CA");
    let csr =
        xp::cluster_identity::generate_node_keypair_and_csr(xp_test_fixtures::secondary_node_id())
            .expect("node CSR");
    let node_cert = xp::cluster_identity::sign_node_csr(
        xp_test_fixtures::primary_cluster_id(),
        &ca.key_pem,
        &csr.csr_pem,
    )
    .expect("node certificate");
    let canary = spawn_signed_tls_server(&ca.key_pem, &ca.cert_pem).await;
    let keypair = generate_reality_keypair(&mut OsRng);
    let endpoint = Endpoint {
        endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port: vless_port,
        meta: serde_json::to_value(VlessRealityVisionTcpEndpointMeta {
            reality: RealityConfig {
                dest: format!("host.docker.internal:{}", canary.addr.port()),
                server_names: xp_test_fixtures::loopback_server_names(),
                server_names_source: RealityServerNamesSource::Manual,
                fingerprint: "chrome".to_string(),
            },
            reality_keys: RealityKeys {
                private_key: keypair.private_key,
                public_key: keypair.public_key,
            },
            short_ids: xp_test_fixtures::endpoint_short_ids(),
            active_short_id: xp_test_fixtures::endpoint_active_short_id().to_owned(),
            canary_upstream: xp_test_fixtures::none(),
            accepted_authorities: xp_test_fixtures::secondary_server_names(),
            mihomo_smux: MihomoSmuxConfig::default(),
            managed_default: true,
        })
        .expect("serialize endpoint metadata"),
    };
    let mut xray = xray::connect(xray_api_addr)
        .await
        .expect("connect Xray API");
    xray.add_inbound(xp::xray::builder::build_add_inbound_request(&endpoint).unwrap())
        .await
        .expect("add Xray Reality inbound");
    wait_for_inbound(SocketAddr::from(([127, 0, 0, 1], vless_port))).await;

    let proxy = spawn_counting_proxy(SocketAddr::from(([127, 0, 0, 1], vless_port))).await;
    let target = mesh_target(&proxy);
    let client = build_mesh_http_client(
        &ca.cert_pem,
        &node_cert,
        &csr.key_pem,
        None,
        MeshProxyStateHandle::disabled(),
    )
    .expect("Mesh client");

    for index in 0..32 {
        let response = client
            .send_peer_request(&target, mesh_request(index), &ca.key_pem, &ca.cert_pem)
            .await
            .expect("sequential Mesh request through Reality fallback");
        assert_eq!(response.version(), Version::HTTP_2);
    }
    let responses = join_all((32..48).map(|index| {
        let client = client.clone();
        let target = target.clone();
        let ca_key = ca.key_pem.clone();
        let ca_cert = ca.cert_pem.clone();
        async move {
            client
                .send_peer_request(&target, mesh_request(index), &ca_key, &ca_cert)
                .await
        }
    }))
    .await;
    for response in responses {
        assert_eq!(
            response.expect("parallel Mesh response").version(),
            Version::HTTP_2
        );
    }
    assert_eq!(proxy.accepts.load(Ordering::SeqCst), 1);

    proxy.disconnect_all().await;
    let response = client
        .send_peer_request(&target, mesh_request(48), &ca.key_pem, &ca.cert_pem)
        .await
        .expect("Mesh reconnect through Reality fallback");
    assert_eq!(response.version(), Version::HTTP_2);
    sleep(Duration::from_millis(20)).await;
    assert_eq!(proxy.accepts.load(Ordering::SeqCst), 2);
    assert!(proxy.peak_active.load(Ordering::SeqCst) <= 2);
    assert_eq!(proxy.active.load(Ordering::SeqCst), 1);

    xray.remove_inbound(
        xp::xray::proto::xray::app::proxyman::command::RemoveInboundRequest { tag: endpoint.tag },
    )
    .await
    .expect("remove Xray Reality inbound");
}
