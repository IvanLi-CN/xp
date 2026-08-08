use std::{
    fs::{self, File},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{StatusCode, Version},
    response::Response,
    routing::any,
};
use rcgen::{CertificateParams, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256};
use tokio::{io::copy_bidirectional, net::TcpListener, task::JoinHandle, time::sleep};
use xp::{
    cluster_metadata::ClusterMetadata,
    domain::{Endpoint, EndpointKind, Node},
    internal_auth,
    protocol::{
        MihomoSmuxConfig, RealityConfig, RealityKeys, RealityServerNamesSource,
        VlessRealityVisionTcpEndpointMeta, generate_reality_keypair, generate_short_id_16hex,
    },
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
};

const PEER_COUNT: usize = 50;
const BODY_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct ResourceRun {
    pub xp_peak_pss_kib: u64,
    pub stack_peak_pss_kib: u64,
    pub cpu_ticks: u64,
    pub tls_accepts: usize,
    pub non_h2_requests: usize,
    pub requests_per_peer: Vec<usize>,
    pub active_per_peer: Vec<usize>,
    pub peak_active_per_peer: Vec<usize>,
}

#[derive(Clone)]
struct PeerServerState {
    ca_key_pem: String,
    ca_cert_pem: String,
    cluster_id: String,
    target_id: String,
    requests: Arc<AtomicUsize>,
    non_h2_requests: Arc<AtomicUsize>,
}

struct PeerConnectionCounters {
    accepts: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    peak_active: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    non_h2_requests: Arc<AtomicUsize>,
}

struct PeerTarget {
    node_id: String,
    access_host: String,
    port: u16,
}

struct PeerFleet {
    targets: Vec<PeerTarget>,
    counters: Vec<PeerConnectionCounters>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for PeerFleet {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn signed_response(State(state): State<PeerServerState>, request: Request) -> Response<Body> {
    let (parts, body) = request.into_parts();
    state.requests.fetch_add(1, Ordering::SeqCst);
    if parts.version != Version::HTTP_2 {
        state.non_h2_requests.fetch_add(1, Ordering::SeqCst);
    }
    let body = match to_bytes(body, BODY_LIMIT).await {
        Ok(body) => body,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap();
        }
    };
    let verified = match internal_auth::verify_request_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &parts.method,
        &parts.uri,
        &parts.headers,
        &body,
        &state.cluster_id,
        &state.target_id,
    ) {
        Ok(verified) => verified,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::empty())
                .unwrap();
        }
    };
    let ack = internal_auth::sign_ack_v2(
        &state.ca_key_pem,
        &state.ca_cert_pem,
        &verified,
        &state.target_id,
        StatusCode::OK.as_u16(),
    )
    .expect("sign Mesh acknowledgement");
    Response::builder()
        .status(StatusCode::OK)
        .header(internal_auth::INTERNAL_ACK_HEADER, ack)
        .body(Body::empty())
        .expect("signed response")
}

async fn spawn_peer_fleet(cluster: &ClusterMetadata, data_dir: &Path) -> PeerFleet {
    let ca_key_pem = cluster
        .read_cluster_ca_key_pem(data_dir)
        .expect("read cluster CA key")
        .expect("bootstrap node CA key");
    let ca_cert_pem = cluster
        .read_cluster_ca_pem(data_dir)
        .expect("read cluster CA certificate");
    let access_hosts = (0..PEER_COUNT)
        .map(|index| format!("127.0.0.{}", index + 10))
        .collect::<Vec<_>>();
    let ca_key = KeyPair::from_pem(&ca_key_pem).expect("parse cluster CA key");
    let ca = Issuer::from_ca_cert_pem(&ca_cert_pem, ca_key).expect("parse cluster CA");
    let cert_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("server key");
    let cert = CertificateParams::new(access_hosts.clone())
        .expect("server certificate params")
        .signed_by(&cert_key, &ca)
        .expect("server certificate");
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert.pem().into_bytes(),
        cert_key.serialize_pem().into_bytes(),
    )
    .await
    .expect("TLS config");

    let mut targets = Vec::with_capacity(PEER_COUNT);
    let mut counters = Vec::with_capacity(PEER_COUNT);
    let mut tasks = Vec::with_capacity(PEER_COUNT * 2);
    for (index, access_host) in access_hosts.into_iter().enumerate() {
        let node_id = xp::id::new_ulid_string();
        let requests = Arc::new(AtomicUsize::new(0));
        let non_h2_requests = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .fallback(any(signed_response))
            .layer(axum::extract::DefaultBodyLimit::disable())
            .with_state(PeerServerState {
                ca_key_pem: ca_key_pem.clone(),
                ca_cert_pem: ca_cert_pem.clone(),
                cluster_id: cluster.cluster_id.clone(),
                target_id: node_id.clone(),
                requests: requests.clone(),
                non_h2_requests: non_h2_requests.clone(),
            });
        let server_listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("peer TLS server listener");
        server_listener
            .set_nonblocking(true)
            .expect("nonblocking TLS listener");
        let server_addr = server_listener.local_addr().expect("TLS server address");
        let server = axum_server::from_tcp_rustls(server_listener, tls.clone())
            .expect("peer TLS server")
            .serve(app.into_make_service());
        tasks.push(tokio::spawn(async move {
            let _ = server.into_future().await;
        }));

        let bind_ip: IpAddr = access_host.parse().expect("loopback peer IP");
        let proxy_listener = TcpListener::bind(SocketAddr::new(bind_ip, 0))
            .await
            .expect("peer counting proxy");
        let proxy_addr = proxy_listener.local_addr().expect("proxy address");
        let accepts = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let peak_active = Arc::new(AtomicUsize::new(0));
        let task_accepts = accepts.clone();
        let task_active = active.clone();
        let task_peak = peak_active.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                let Ok((mut downstream, _)) = proxy_listener.accept().await else {
                    break;
                };
                task_accepts.fetch_add(1, Ordering::SeqCst);
                let active = task_active.clone();
                let peak_active = task_peak.clone();
                tokio::spawn(async move {
                    let Ok(mut upstream) = tokio::net::TcpStream::connect(server_addr).await else {
                        return;
                    };
                    let active_now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak_active.fetch_max(active_now, Ordering::SeqCst);
                    let _ = copy_bidirectional(&mut downstream, &mut upstream).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        }));
        targets.push(PeerTarget {
            node_id,
            access_host,
            port: proxy_addr.port(),
        });
        counters.push(PeerConnectionCounters {
            accepts,
            active,
            peak_active,
            requests,
            non_h2_requests,
        });
        assert_eq!(targets.len(), index + 1);
    }
    PeerFleet {
        targets,
        counters,
        tasks,
    }
}

fn reserve_local_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("reserve XP bind port")
        .local_addr()
        .expect("XP bind address")
        .port()
}

fn run_init(binary: &Path, data_dir: &Path, bind_port: u16) {
    let status = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().expect("UTF-8 data dir"),
            "--node-name",
            "mesh-resource-caller",
            "--access-host",
            "caller.mesh.test",
            "--api-base-url",
            &format!("https://127.0.0.1:{bind_port}"),
            "init",
        ])
        .status()
        .expect("run xp init");
    assert!(status.success(), "xp init failed with {status}");
}

fn prepare_peer_state(data_dir: &Path, cluster: &ClusterMetadata, fleet: &PeerFleet) {
    xp::internal_auth_epoch::ensure_startup_epoch(data_dir, 1).expect("initialize auth epoch");
    let mut store = JsonSnapshotStore::load_or_init(StoreInit {
        data_dir: data_dir.to_path_buf(),
        bootstrap_node_id: Some(cluster.node_id.clone()),
        bootstrap_node_name: cluster.node_name.clone(),
        bootstrap_access_host: cluster.access_host.clone(),
        bootstrap_api_base_url: cluster.api_base_url.clone(),
    })
    .expect("load resource state");
    let mut rng = rand::rngs::OsRng;
    let reality_keypair = generate_reality_keypair(&mut rng);
    let short_id = generate_short_id_16hex(&mut rng);
    let meta = serde_json::to_value(VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: "example.com:443".to_string(),
            server_names: vec!["example.com".to_string()],
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: "chrome".to_string(),
        },
        reality_keys: RealityKeys {
            private_key: reality_keypair.private_key,
            public_key: reality_keypair.public_key,
        },
        short_ids: vec![short_id.clone()],
        active_short_id: short_id,
        canary_upstream: None,
        accepted_authorities: Vec::new(),
        mihomo_smux: MihomoSmuxConfig::default(),
        managed_default: true,
    })
    .expect("serialize managed endpoint metadata");
    for (index, target) in fleet.targets.iter().enumerate() {
        DesiredStateCommand::UpsertNode {
            node: Node {
                node_id: target.node_id.clone(),
                node_name: format!("mesh-resource-peer-{index:02}"),
                access_host: target.access_host.clone(),
                api_base_url: "http://127.0.0.1:9".to_string(),
                quota_limit_bytes: 0,
                quota_reset: Default::default(),
            },
        }
        .apply(store.state_mut())
        .expect("insert resource peer");
        DesiredStateCommand::UpsertEndpoint {
            endpoint: Endpoint {
                endpoint_id: xp::id::new_ulid_string(),
                node_id: target.node_id.clone(),
                tag: format!("vless-mesh-resource-{index:02}"),
                kind: EndpointKind::VlessRealityVisionTcp,
                port: target.port,
                meta: meta.clone(),
            },
        }
        .apply(store.state_mut())
        .expect("insert resource peer endpoint");
    }
    store.save().expect("persist resource state");
}

fn spawn_xp(binary: &Path, data_dir: &Path, bind_port: u16, label: &str) -> Child {
    let log_path = data_dir.join(format!("{label}.log"));
    let stdout = File::create(&log_path).expect("create XP resource log");
    let stderr = stdout.try_clone().expect("clone XP resource log");
    let admin_hash =
        xp::admin_token::hash_admin_token_argon2id("mesh-resource-test-token-0000000000000000")
            .expect("hash test admin token");
    Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().expect("UTF-8 data dir"),
            "--bind",
            &format!("127.0.0.1:{bind_port}"),
            "--xray-api-addr",
            "127.0.0.1:9",
            "--xray-health-interval-secs",
            "30",
            "run",
        ])
        .env("XP_ADMIN_TOKEN_HASH", admin_hash.as_str())
        .env("RUST_LOG", "error")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn XP resource candidate")
}

async fn wait_for_xp(child: &mut Child, bind_port: u16, log_path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().expect("poll XP child") {
            let log = fs::read_to_string(log_path).unwrap_or_default();
            panic!("XP exited before readiness with {status}:\n{log}");
        }
        if tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, bind_port))
            .await
            .is_ok()
        {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for XP");
        sleep(Duration::from_millis(100)).await;
    }
}

fn read_pss_kib(pid: u32) -> Option<u64> {
    let rollup = PathBuf::from(format!("/proc/{pid}/smaps_rollup"));
    let fallback = PathBuf::from(format!("/proc/{pid}/smaps"));
    let raw = fs::read_to_string(if rollup.exists() { rollup } else { fallback }).ok()?;
    let pss = raw
        .lines()
        .filter_map(|line| line.strip_prefix("Pss:"))
        .filter_map(|value| value.split_whitespace().next())
        .filter_map(|value| value.parse::<u64>().ok())
        .sum();
    (pss > 0).then_some(pss)
}

fn read_cpu_ticks(pid: u32) -> u64 {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).expect("read process stat");
    let fields = stat
        .split_once(") ")
        .expect("process stat comm delimiter")
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let user = fields[11].parse::<u64>().expect("user CPU ticks");
    let system = fields[12].parse::<u64>().expect("system CPU ticks");
    user + system
}

async fn stop_child(child: &mut Child) {
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGINT);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().expect("poll stopped XP").is_some() {
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
    child.kill().expect("kill XP after grace period");
    let _ = child.wait();
}

pub fn support_pids_from_env() -> Vec<u32> {
    std::env::var("XP_MESH_RESOURCE_SUPPORT_PIDS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|value| value.trim().parse::<u32>().ok())
        .collect()
}

pub async fn run_resource_workload(
    label: &str,
    binary: &Path,
    duration: Duration,
    support_pids: &[u32],
) -> ResourceRun {
    let temp = tempfile::tempdir().expect("resource data directory");
    let bind_port = reserve_local_port();
    run_init(binary, temp.path(), bind_port);
    let cluster = ClusterMetadata::load(temp.path()).expect("load initialized cluster");
    let fleet = spawn_peer_fleet(&cluster, temp.path()).await;
    prepare_peer_state(temp.path(), &cluster, &fleet);
    let log_path = temp.path().join(format!("{label}.log"));
    let mut child = spawn_xp(binary, temp.path(), bind_port, label);
    wait_for_xp(&mut child, bind_port, &log_path).await;
    let pid = child.id();
    let cpu_started = read_cpu_ticks(pid);
    let mut xp_peak_pss_kib = 0;
    let mut stack_peak_pss_kib = 0;
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("poll XP workload") {
            let log = fs::read_to_string(&log_path).unwrap_or_default();
            panic!("XP exited during {label} workload with {status}:\n{log}");
        }
        let xp_pss = read_pss_kib(pid).expect("read XP PSS");
        let support_pss = support_pids
            .iter()
            .map(|pid| {
                read_pss_kib(*pid).unwrap_or_else(|| panic!("read support process {pid} PSS"))
            })
            .sum::<u64>();
        xp_peak_pss_kib = xp_peak_pss_kib.max(xp_pss);
        stack_peak_pss_kib = stack_peak_pss_kib.max(xp_pss.saturating_add(support_pss));
        sleep(Duration::from_secs(1)).await;
    }
    let cpu_ticks = read_cpu_ticks(pid).saturating_sub(cpu_started);
    let tls_accepts = fleet
        .counters
        .iter()
        .map(|counter| counter.accepts.load(Ordering::SeqCst))
        .sum();
    let non_h2_requests = fleet
        .counters
        .iter()
        .map(|counter| counter.non_h2_requests.load(Ordering::SeqCst))
        .sum();
    let requests_per_peer = fleet
        .counters
        .iter()
        .map(|counter| counter.requests.load(Ordering::SeqCst))
        .collect();
    let active_per_peer = fleet
        .counters
        .iter()
        .map(|counter| counter.active.load(Ordering::SeqCst))
        .collect();
    let peak_active_per_peer = fleet
        .counters
        .iter()
        .map(|counter| counter.peak_active.load(Ordering::SeqCst))
        .collect();
    stop_child(&mut child).await;
    ResourceRun {
        xp_peak_pss_kib,
        stack_peak_pss_kib,
        cpu_ticks,
        tls_accepts,
        non_h2_requests,
        requests_per_peer,
        active_per_peer,
        peak_active_per_peer,
    }
}
