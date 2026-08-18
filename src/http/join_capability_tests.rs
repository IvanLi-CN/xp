use std::{
    collections::BTreeSet,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode, Uri, header},
    response::Response,
    routing::any,
};
use rcgen::{CertificateParams, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256};
use tempfile::TempDir;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};
use tower::util::ServiceExt;

use crate::{
    cloudflared_supervisor::{CloudflaredHealthHandle, CloudflaredStatus},
    cluster_metadata::ClusterMetadata,
    config::{Config, XrayRestartMode},
    control_plane_mesh::MeshAwareHttpClient,
    ddns::{DdnsHealthHandle, DdnsStatus},
    domain::{Node, NodeQuotaReset},
    http::build_router_with_mesh_telemetry,
    id::new_ulid_string,
    internal_auth::{self, InternalRoute, RequestContext},
    managed_default_endpoints::{
        DEFAULT_VLESS_FINGERPRINT, DefaultVlessEndpointSpec, build_managed_default_vless_endpoint,
    },
    mesh_telemetry::MeshTelemetryHandle,
    protocol::RealityServerNamesSource,
    raft::{
        app::{LocalRaft, RaftFacade},
        types::{NodeMeta as RaftNodeMeta, raft_node_id_from_ulid},
    },
    reconcile::ReconcileHandle,
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
    xray_supervisor::XrayHealthHandle,
};

const CAPABILITIES_PATH: &str = "/api/admin/_internal/capabilities";
const MESH_HEALTH_PATH: &str = "/api/admin/_internal/mesh/health";

#[derive(Clone)]
struct MeshCapabilityServerState {
    ca_key_pem: String,
    ca_cert_pem: String,
    expected_cluster_id: String,
    sender_id: String,
    target_id: String,
    capability_requests: Arc<AtomicUsize>,
}

async fn mesh_capability_response(
    State(state): State<MeshCapabilityServerState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let verified = internal_auth::verify_request_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &method,
        &uri,
        &headers,
        &body,
        &state.expected_cluster_id,
        &state.target_id,
    )
    .expect("valid signed Mesh request");
    assert_eq!(verified.context.sender_id, state.sender_id);

    let response_body = match uri.path() {
        CAPABILITIES_PATH => {
            assert_eq!(method, Method::GET);
            assert_eq!(verified.context.route, InternalRoute::MeshV2);
            state.capability_requests.fetch_add(1, Ordering::SeqCst);
            r#"{"capabilities":["cluster.membership-lifecycle-v1"]}"#
        }
        MESH_HEALTH_PATH => {
            assert_eq!(method, Method::GET);
            assert_eq!(verified.context.route, InternalRoute::HealthV2);
            r#"{"ok":true}"#
        }
        path => panic!("unexpected Mesh path: {path}"),
    };
    let acknowledgement = internal_auth::sign_ack_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &verified,
        &state.target_id,
        StatusCode::OK.as_u16(),
    )
    .expect("sign Mesh acknowledgement");

    Response::builder()
        .status(StatusCode::OK)
        .header(internal_auth::INTERNAL_ACK_HEADER, acknowledgement)
        .body(Body::from(response_body))
        .expect("Mesh response")
}

async fn spawn_mesh_capability_server(
    state: MeshCapabilityServerState,
    mesh_host: &str,
) -> (SocketAddr, JoinHandle<()>) {
    let ca_key = KeyPair::from_pem(&state.ca_key_pem).expect("cluster CA key");
    let ca = Issuer::from_ca_cert_pem(&state.ca_cert_pem, ca_key).expect("cluster CA certificate");
    let server_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("server key");
    let certificate = CertificateParams::new(vec![mesh_host.to_string()])
        .expect("certificate parameters")
        .signed_by(&server_key, &ca)
        .expect("server certificate");
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
        certificate.pem().into_bytes(),
        server_key.serialize_pem().into_bytes(),
    )
    .await
    .expect("TLS configuration");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Mesh listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let address = listener.local_addr().expect("Mesh listener address");
    let server = axum_server::from_tcp_rustls(listener, tls)
        .expect("Mesh server")
        .serve(
            Router::new()
                .fallback(any(mesh_capability_response))
                .with_state(state)
                .into_make_service(),
        );
    let task = tokio::spawn(async move {
        let _ = server.into_future().await;
    });
    (address, task)
}

fn mesh_client(
    cluster_ca_pem: &str,
    node_cert_pem: &str,
    node_key_pem: &str,
    mesh_host: &str,
    mesh_address: SocketAddr,
) -> MeshAwareHttpClient {
    let identity_pem = format!("{node_cert_pem}\n{node_key_pem}");
    let mesh = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(cluster_ca_pem.as_bytes()).expect("cluster CA"),
        )
        .identity(reqwest::Identity::from_pem(identity_pem.as_bytes()).expect("node identity"))
        .resolve(mesh_host, mesh_address)
        .http2_prior_knowledge()
        .http2_adaptive_window(true)
        .build()
        .expect("Mesh client");
    let public = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(cluster_ca_pem.as_bytes()).expect("cluster CA"),
        )
        .identity(reqwest::Identity::from_pem(identity_pem.as_bytes()).expect("node identity"))
        .build()
        .expect("public client");
    MeshAwareHttpClient::from_transport_clients(mesh, public)
}

fn test_config(data_dir: PathBuf) -> Config {
    Config {
        bind: xp_test_fixtures::address_loopback_port0().parse().unwrap(),
        xray_api_addr: SocketAddr::from(([127, 0, 0, 1], 10085)),
        xray_health_interval_secs: 5,
        xray_health_fails_before_down: 4,
        xray_restart_mode: XrayRestartMode::None,
        xray_restart_cooldown_secs: 30,
        xray_restart_timeout_secs: 20,
        xray_systemd_unit: "xray.service".to_string(),
        xray_openrc_service: "xray".to_string(),
        cloudflared_health_interval_secs: 5,
        cloudflared_health_fails_before_down: 3,
        cloudflared_monitor_mode: Some(XrayRestartMode::None),
        cloudflared_restart_mode: XrayRestartMode::None,
        cloudflared_restart_cooldown_secs: 30,
        cloudflared_restart_timeout_secs: 20,
        cloudflared_systemd_unit: "cloudflared.service".to_string(),
        cloudflared_openrc_service: "cloudflared".to_string(),
        data_dir,
        admin_token_hash: String::new(),
        node_name: xp_test_fixtures::label_node1_variant2().to_owned(),
        access_host: xp_test_fixtures::label_empty().to_owned(),
        api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
        vless_canary_bind: SocketAddr::from((
            [127, 0, 0, 1],
            crate::config::DEFAULT_VLESS_CANARY_BIND_PORT,
        )),
        vless_canary_acme_directory_url: "https://acme-v02.api.letsencrypt.org/directory"
            .to_string(),
        vless_canary_acme_contact_email: String::new(),
        vless_canary_cloudflare_token_file: crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE
            .to_string(),
        vless_canary_cloudflare_zone_id: String::new(),
        vless_canary_dns_propagation_timeout_secs: 180,
        default_vless_port: None,
        default_vless_server_names: None,
        default_vless_fingerprint: None,
        default_ss_port: None,
        cloudflare_ddns_enabled: false,
        cloudflare_ddns_token_file: crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE.to_string(),
        cloudflare_ddns_zone_id: String::new(),
        cloudflare_ddns_ipv4_url: crate::public_ip_probe::DEFAULT_TRACE_URL.to_string(),
        cloudflare_ddns_ipv6_url: crate::public_ip_probe::DEFAULT_TRACE_URL.to_string(),
        cloudflare_ddns_interval_secs_with_monitor: 300,
        cloudflare_ddns_interval_secs_no_monitor: 60,
        cloudflare_ddns_fast_interval_secs: 30,
        cloudflare_ddns_fast_window_secs: 300,
        cloudflare_ddns_family_missing_grace: 3,
        endpoint_probe_skip_self_test: false,
        quota_poll_interval_secs: 10,
        quota_auto_unban: true,
        ip_geo_enabled: false,
        ip_geo_origin: "https://api.country.is".to_string(),
    }
}

fn build_test_router(
    config: Config,
    cluster: ClusterMetadata,
    store: Arc<Mutex<JsonSnapshotStore>>,
    raft: Arc<dyn RaftFacade>,
    mesh_client: MeshAwareHttpClient,
) -> Router {
    let cluster_ca_pem = cluster.read_cluster_ca_pem(&config.data_dir).unwrap();
    let cluster_ca_key_pem = cluster.read_cluster_ca_key_pem(&config.data_dir).unwrap();
    let xray_health = XrayHealthHandle::new_unknown();
    let cloudflared_health = CloudflaredHealthHandle::new_with_status(CloudflaredStatus::Disabled);
    let ddns_health = DdnsHealthHandle::new_with_status(DdnsStatus::Disabled);
    let (node_runtime, _node_runtime_task) = crate::node_runtime::spawn_node_runtime_monitor(
        Arc::new(config.clone()),
        cluster.node_id.clone(),
        xray_health.clone(),
        cloudflared_health.clone(),
        ddns_health,
    );
    let endpoint_probe = crate::endpoint_probe::new_endpoint_probe_handle(
        cluster.node_id.clone(),
        store.clone(),
        raft.clone(),
        "test-probe-secret".to_string(),
        false,
    );
    let (geo_db_update, _geo_db_update_task) =
        crate::ip_geo_db::spawn_geo_db_update_worker(Arc::new(config.clone()), store.clone())
            .unwrap();
    let mesh_telemetry = MeshTelemetryHandle::load(&config.data_dir).unwrap();

    build_router_with_mesh_telemetry(
        config.clone(),
        store.clone(),
        ReconcileHandle::noop(),
        xray_health,
        cloudflared_health,
        node_runtime,
        crate::node_history::NodeHistoryHandle::from_config(&config),
        endpoint_probe,
        crate::node_egress_probe::NodeEgressProbeHandle::new_noop(cluster.node_id.clone(), store),
        cluster,
        cluster_ca_pem,
        cluster_ca_key_pem,
        raft,
        None,
        geo_db_update,
        mesh_telemetry,
        mesh_client,
    )
}

#[tokio::test]
async fn orphan_repair_dry_run_uses_mesh_when_public_urls_are_unavailable() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let tmp = TempDir::new().unwrap();
    let config = test_config(tmp.path().to_path_buf());
    let cluster = ClusterMetadata::init_new_cluster(
        tmp.path(),
        config.node_name.clone(),
        config.access_host.clone(),
        config.api_base_url.clone(),
    )
    .unwrap();
    let cluster_ca_pem = cluster.read_cluster_ca_pem(tmp.path()).unwrap();
    let cluster_ca_key_pem = cluster
        .read_cluster_ca_key_pem(tmp.path())
        .unwrap()
        .expect("cluster CA key");
    let remote_node = Node {
        node_id: xp_test_fixtures::identifier_ulid_a().to_owned(),
        node_name: xp_test_fixtures::label_node_remote().to_owned(),
        access_host: xp_test_fixtures::host_fixture575().to_owned(),
        api_base_url: xp_test_fixtures::url_loopback1().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    let requests = Arc::new(AtomicUsize::new(0));
    let (mesh_address, mesh_server) = spawn_mesh_capability_server(
        MeshCapabilityServerState {
            ca_key_pem: cluster_ca_key_pem.clone(),
            ca_cert_pem: cluster_ca_pem.clone(),
            expected_cluster_id: cluster.cluster_id.clone(),
            sender_id: cluster.node_id.clone(),
            target_id: remote_node.node_id.clone(),
            capability_requests: requests.clone(),
        },
        xp_test_fixtures::host_fixture575(),
    )
    .await;
    tokio::task::yield_now().await;

    let store = JsonSnapshotStore::load_or_init(StoreInit {
        data_dir: config.data_dir.clone(),
        bootstrap_node_id: Some(cluster.node_id.clone()),
        bootstrap_node_name: config.node_name.clone(),
        bootstrap_access_host: config.access_host.clone(),
        bootstrap_api_base_url: config.api_base_url.clone(),
    })
    .unwrap();
    let store = Arc::new(Mutex::new(store));
    let orphan_node_id = raft_node_id_from_ulid(xp_test_fixtures::identifier_ulid_b()).unwrap();
    {
        let mut locked = store.lock().await;
        locked.upsert_node(remote_node.clone()).unwrap();
        let endpoint = build_managed_default_vless_endpoint(
            &DefaultVlessEndpointSpec {
                port: mesh_address.port(),
                reality_dest: "origin.example.test:443".to_string(),
                server_names: xp_test_fixtures::host_list_edge1(),
                server_names_source: RealityServerNamesSource::Manual,
                fingerprint: DEFAULT_VLESS_FINGERPRINT.to_string(),
            },
            remote_node.node_id.clone(),
        )
        .unwrap();
        DesiredStateCommand::UpsertEndpoint {
            endpoint,
            expected: None,
        }
        .apply(locked.state_mut())
        .unwrap();
    }

    let local_node_id = raft_node_id_from_ulid(&cluster.node_id).unwrap();
    let remote_raft_node_id = raft_node_id_from_ulid(&remote_node.node_id).unwrap();
    let mut metrics = openraft::RaftMetrics::new_initial(local_node_id);
    metrics.current_term = 1;
    metrics.state = openraft::ServerState::Leader;
    metrics.current_leader = Some(local_node_id);
    let mut nodes = std::collections::BTreeMap::new();
    nodes.insert(
        local_node_id,
        RaftNodeMeta {
            name: cluster.node_name.clone(),
            api_base_url: xp_test_fixtures::url_loopback62416().to_owned(),
            raft_endpoint: xp_test_fixtures::url_loopback62416().to_owned(),
        },
    );
    nodes.insert(
        remote_raft_node_id,
        RaftNodeMeta {
            name: remote_node.node_name.clone(),
            api_base_url: xp_test_fixtures::url_loopback1().to_owned(),
            raft_endpoint: xp_test_fixtures::url_loopback1().to_owned(),
        },
    );
    nodes.insert(
        orphan_node_id,
        RaftNodeMeta {
            name: "stale-orphan".to_string(),
            api_base_url: xp_test_fixtures::url_loopback1().to_owned(),
            raft_endpoint: xp_test_fixtures::url_loopback1().to_owned(),
        },
    );
    let membership = openraft::Membership::new(
        vec![BTreeSet::from([
            local_node_id,
            remote_raft_node_id,
            orphan_node_id,
        ])],
        nodes,
    );
    metrics.membership_config = Arc::new(openraft::StoredMembership::new(None, membership));
    let (_metrics_tx, metrics_rx) = watch::channel(metrics);
    let raft: Arc<dyn RaftFacade> = Arc::new(LocalRaft::new(store.clone(), metrics_rx));
    let app = build_test_router(
        config,
        cluster.clone(),
        store,
        raft,
        mesh_client(
            &cluster_ca_pem,
            &cluster.read_node_cert_pem(tmp.path()).unwrap(),
            &cluster.read_node_key_pem(tmp.path()).unwrap(),
            xp_test_fixtures::host_fixture575(),
            mesh_address,
        ),
    );

    let uri: Uri = "/api/admin/_internal/raft/repair-orphan-voter"
        .parse()
        .unwrap();
    let body = serde_json::to_vec(&serde_json::json!({
        "raft_node_id": orphan_node_id,
        "apply": false,
    }))
    .unwrap();
    let context = RequestContext::now(
        InternalRoute::MeshV2,
        &cluster.cluster_id,
        &cluster.node_id,
        &cluster.node_id,
        new_ulid_string(),
    );
    let mut headers = HeaderMap::new();
    internal_auth::sign_request_v2(
        &cluster_ca_key_pem,
        &cluster_ca_pem,
        &Method::POST,
        &uri,
        Some("application/json"),
        &body,
        &context,
        &mut headers,
    )
    .unwrap();
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap();
    request.headers_mut().extend(headers);

    let response = app.oneshot(request).await.unwrap();
    mesh_server.abort();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 8 * 1024)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["dry_run"], true);
    assert_eq!(body["raft_node_id"], orphan_node_id);
    assert!(body["expected_membership"].as_str().is_some());
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}
