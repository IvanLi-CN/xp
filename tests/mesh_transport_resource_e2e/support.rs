use std::{
    fs::{self, File},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
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
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rcgen::{CertificateParams, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256};
use tokio::{io::copy_bidirectional, net::TcpListener, task::JoinHandle, time::sleep};
use xp::{
    cluster_metadata::ClusterMetadata,
    domain::{Endpoint, EndpointKind, Node},
    internal_auth,
    protocol::{
        MihomoSmuxConfig, RealityConfig, RealityKeys, RealityServerNamesSource,
        VlessRealityVisionTcpEndpointMeta, generate_reality_keypair,
    },
    state::{DesiredStateCommand, JsonSnapshotStore, StoreInit},
};

const PEER_COUNT: usize = 50;
const BODY_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct ResourceRun {
    pub xp_peak_pss_kib: u64,
    pub xp_peak_anon_pss_kib: u64,
    pub xp_peak_file_pss_kib: u64,
    pub stack_peak_pss_kib: u64,
    pub cpu_ticks: u64,
    pub tls_accepts: usize,
    pub non_h2_requests: usize,
    pub requests_per_peer: Vec<usize>,
    pub active_per_peer: Vec<usize>,
    pub peak_active_per_peer: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
struct PssSample {
    total_kib: u64,
    anon_kib: u64,
    file_kib: u64,
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
    let meta = serde_json::to_value(VlessRealityVisionTcpEndpointMeta {
        reality: RealityConfig {
            dest: xp_test_fixtures::primary_authority().to_owned(),
            server_names: xp_test_fixtures::primary_server_names(),
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: "chrome".to_string(),
        },
        reality_keys: RealityKeys {
            private_key: reality_keypair.private_key,
            public_key: reality_keypair.public_key,
        },
        short_ids: xp_test_fixtures::endpoint_short_ids(),
        active_short_id: xp_test_fixtures::endpoint_active_short_id().to_owned(),
        canary_upstream: xp_test_fixtures::none(),
        accepted_authorities: xp_test_fixtures::secondary_server_names(),
        mihomo_smux: MihomoSmuxConfig::default(),
        transport: Default::default(),
        managed_default: true,
    })
    .expect("serialize managed endpoint metadata");
    for (index, target) in fleet.targets.iter().enumerate() {
        DesiredStateCommand::UpsertNode {
            node: Node {
                node_id: target.node_id.clone(),
                node_name: xp_test_fixtures::primary_node_name().to_owned(),
                access_host: target.access_host.clone(),
                api_base_url: "http://127.0.0.1:9".to_string(),
                quota_limit_bytes: 0,
                quota_reset: Default::default(),
            },
            join_session: None,
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
            expected: None,
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

fn prepare_summary_storage(data_dir: &Path, cluster: &ClusterMetadata) {
    let connection = rusqlite::Connection::open(data_dir.join("history.sqlite3"))
        .expect("open summary resource database");
    connection
        .pragma_update(None, "cache_size", -1024_i64)
        .expect("limit summary resource SQLite cache");
    connection
        .pragma_update(None, "wal_autocheckpoint", 100_i64)
        .expect("limit summary resource WAL checkpoint");
    let transaction = connection
        .unchecked_transaction()
        .expect("begin summary resource transaction");
    let now_unix_seconds = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
    let first_observed = now_unix_seconds.saturating_sub(60);
    for sequence in 0..257_u64 {
        let observed = first_observed.saturating_add(sequence);
        transaction
            .execute(
                "INSERT INTO repository_history_segments
                     (id, closed_at, contains_tombstone, source_node_id, source_epoch,
                      stream, first_sequence, payload)
                 VALUES (?1, ?2, 0, 'summary-source', 1, 'runtime', ?3, zeroblob(?4))",
                rusqlite::params![
                    format!("{sequence:064x}"),
                    observed,
                    sequence,
                    192 * 1024 - 1024
                ],
            )
            .expect("insert summary resource segment");

        let record_payload = serde_json::to_vec(&serde_json::json!({
            "observed_at_unix_seconds": observed,
            "received_at_unix_seconds": observed,
            "source_node_id": "summary-source",
            "source_epoch": 1,
            "stream": "runtime",
            "sequence": sequence,
            "subject_node_id": "summary-subject",
            "observer_node_id": "summary-source",
            "schema_id": "runtime.v1",
            "schema_version": 1,
            "record_key": vec![0_u8; 96 * 1024],
            "payload": [],
            "tombstone": false,
        }))
        .expect("encode summary resource record");
        transaction
            .execute(
                "INSERT INTO repository_history_records
                     (source_node_id, source_epoch, stream, sequence, subject_node_id,
                      observer_node_id, schema_id, schema_version, record_key, is_tombstone,
                      observed_start, observed_end, received_at, aggregate_complete,
                      aggregate_start, aggregate_end, payload)
                 VALUES ('summary-source', 1, 'runtime', ?1, 'summary-subject',
                         'summary-source', 'runtime.v1', 1, ?2, 0, ?3, ?3, ?3,
                         1, NULL, NULL, ?4)",
                rusqlite::params![
                    sequence,
                    format!("record-{sequence}").into_bytes(),
                    observed,
                    record_payload
                ],
            )
            .expect("insert summary resource record");
    }

    let replica_snapshot = serde_json::json!({
        "external_history": true,
        "legacy_segment_cursor_index_complete": true,
        "partition_summaries": [{
            "source_node_id": "summary-source",
            "source_epoch": 1,
            "stream": "runtime",
            "partition": 0,
            "first_sequence": 0,
            "last_sequence": 256,
            "hash": vec![0_u8; 32],
            "record_count": 257,
        }],
        "partition_summary_cursor": {
            "observed_start_unix_seconds": first_observed + 256,
            "source_node_id": "summary-source",
            "source_epoch": 1,
            "stream": "runtime",
            "sequence": 256,
        },
        "partition_summaries_complete": true,
    });
    let replica_payload =
        serde_json::to_vec(&replica_snapshot).expect("encode summary resource replica snapshot");
    transaction
        .execute(
            "INSERT OR REPLACE INTO history_snapshots (key, payload, updated_at)
             VALUES ('repository_replica', ?1, 0)",
            rusqlite::params![replica_payload],
        )
        .expect("write summary resource replica snapshot");

    let state_payload = transaction
        .query_row(
            "SELECT payload FROM history_snapshots WHERE key = 'persistent_state'",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .expect("read initialized state snapshot");
    let mut state: serde_json::Value =
        serde_json::from_slice(&state_payload).expect("decode initialized state snapshot");
    let key = URL_SAFE_NO_PAD.encode([1_u8; 32]);
    let relay_key = URL_SAFE_NO_PAD.encode([2_u8; 32]);
    state["repository_membership"] = serde_json::json!({
        "members": [{
            "identity": {
                "node_id": cluster.node_id.clone(),
                "ed25519_public_key": key,
                "x25519_relay_public_key": relay_key
            },
            "lifecycle": "syncing",
            "replica_converged": false,
            "capacity": {
                "quota_bytes": 10_u64 * 1024 * 1024 * 1024,
                "used_bytes": 0,
                "filesystem_available_bytes": u64::MAX
            }
        }]
    });
    let state_payload = serde_json::to_vec(&state).expect("encode summary resource state");
    transaction
        .execute(
            "UPDATE history_snapshots SET payload = ?1 WHERE key = 'persistent_state'",
            rusqlite::params![state_payload],
        )
        .expect("write summary resource state");
    transaction
        .commit()
        .expect("commit summary resource fixtures");
}

pub async fn run_repository_summary_resource_workload(binary: &Path) -> u64 {
    let temp = tempfile::tempdir().expect("summary resource data directory");
    let bind_port = reserve_local_port();
    run_init(binary, temp.path(), bind_port);
    let cluster = ClusterMetadata::load(temp.path()).expect("load summary resource cluster");
    prepare_summary_storage(temp.path(), &cluster);
    let log_path = temp.path().join("summary.log");
    let mut child = spawn_xp(binary, temp.path(), bind_port, "summary");
    wait_for_xp(&mut child, bind_port, &log_path).await;
    let pid = child.id();
    assert_expected_memory_scope(pid);

    let ca_pem = cluster
        .read_cluster_ca_pem(temp.path())
        .expect("read summary resource CA");
    let ca_key_pem = cluster
        .read_cluster_ca_key_pem(temp.path())
        .expect("read summary resource CA key")
        .expect("summary resource private CA key");
    let uri: axum::http::Uri =
        "/api/admin/_internal/history-repository/summary?deep_verification=true"
            .parse()
            .expect("summary resource URI");
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("summary resource HTTP client");
    let mut max_pss_kib = 0;
    for _ in 0..5 {
        let context = xp::internal_auth::RequestContext::now(
            xp::internal_auth::InternalRoute::MeshV2,
            &cluster.cluster_id,
            &cluster.node_id,
            &cluster.node_id,
            xp::id::new_ulid_string(),
        );
        let mut headers = axum::http::HeaderMap::new();
        xp::internal_auth::sign_request_v2(
            &ca_key_pem,
            &ca_pem,
            &axum::http::Method::GET,
            &uri,
            None,
            &[],
            &context,
            &mut headers,
        )
        .expect("sign summary resource request");
        let mut request = client.get(format!("http://127.0.0.1:{bind_port}{uri}"));
        for (name, value) in &headers {
            request = request.header(name.as_str(), value.to_str().expect("signed header value"));
        }
        let sampling = Arc::new(AtomicBool::new(true));
        let sampled_peak_pss_kib = Arc::new(AtomicU64::new(0));
        let sample_count = Arc::new(AtomicUsize::new(0));
        let sampler_started = Arc::new(AtomicBool::new(false));
        let request_active = Arc::new(AtomicBool::new(false));
        let in_flight_sample_count = Arc::new(AtomicUsize::new(0));
        let sampler_sampling = Arc::clone(&sampling);
        let sampler_peak = Arc::clone(&sampled_peak_pss_kib);
        let sampler_count = Arc::clone(&sample_count);
        let sampler_ready = Arc::clone(&sampler_started);
        let sampler_request_active = Arc::clone(&request_active);
        let sampler_in_flight_count = Arc::clone(&in_flight_sample_count);
        let sampler = tokio::spawn(async move {
            if let Some(sample) = read_pss(pid) {
                sampler_peak.fetch_max(sample.total_kib, Ordering::Relaxed);
                sampler_count.fetch_add(1, Ordering::Relaxed);
            }
            sampler_ready.store(true, Ordering::Release);
            while sampler_sampling.load(Ordering::Relaxed) {
                if let Some(sample) = read_pss(pid) {
                    sampler_peak.fetch_max(sample.total_kib, Ordering::Relaxed);
                    sampler_count.fetch_add(1, Ordering::Relaxed);
                    if sampler_request_active.load(Ordering::Acquire) {
                        sampler_in_flight_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
                tokio::task::yield_now().await;
            }
        });
        while !sampler_started.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        request_active.store(true, Ordering::Release);
        let response = request.send().await.expect("summary resource response");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let summary: serde_json::Value = response.json().await.expect("decode summary response");
        request_active.store(false, Ordering::Release);
        sampling.store(false, Ordering::Relaxed);
        sampler.await.expect("summary PSS sampler");
        assert!(sample_count.load(Ordering::Relaxed) > 0);
        assert!(in_flight_sample_count.load(Ordering::Relaxed) > 0);
        assert_eq!(summary["segment_ids"].as_array().map(Vec::len), Some(256));
        assert_eq!(summary["partitions_included"], serde_json::json!(true));
        assert_eq!(
            summary["partitions"][0]["record_count"],
            serde_json::json!(257)
        );
        assert!(
            summary["next_segment_id"]
                .as_str()
                .is_some_and(|cursor| { cursor.starts_with("r:") })
        );
        max_pss_kib = max_pss_kib
            .max(sampled_peak_pss_kib.load(Ordering::Relaxed))
            .max(read_pss(pid).expect("read summary XP PSS").total_kib);
        sleep(Duration::from_secs(1)).await;
    }
    stop_child(&mut child).await;
    max_pss_kib
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

fn read_pss(pid: u32) -> Option<PssSample> {
    let rollup = PathBuf::from(format!("/proc/{pid}/smaps_rollup"));
    let fallback = PathBuf::from(format!("/proc/{pid}/smaps"));
    let uses_rollup = rollup.exists();
    let raw = fs::read_to_string(if uses_rollup { rollup } else { fallback }).ok()?;
    let metric = |name: &str| {
        raw.lines()
            .filter_map(|line| line.strip_prefix(name))
            .filter_map(|value| value.split_whitespace().next())
            .filter_map(|value| value.parse::<u64>().ok())
            .sum::<u64>()
    };
    let total_kib = metric("Pss:");
    (total_kib > 0).then_some(PssSample {
        total_kib,
        anon_kib: if uses_rollup { metric("Pss_Anon:") } else { 0 },
        file_kib: if uses_rollup { metric("Pss_File:") } else { 0 },
    })
}

fn assert_expected_memory_scope(pid: u32) {
    if std::env::var_os("XP_MESH_RESOURCE_EXPECT_MEMORY_LIMIT").is_none() {
        return;
    }
    let cgroup_contents = fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .expect("read XP cgroup membership")
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .expect("read unified XP cgroup membership")
        .trim_start_matches('/')
        .to_owned();
    let cgroup_dir = Path::new("/sys/fs/cgroup").join(cgroup_contents);
    let memory_max = fs::read_to_string(cgroup_dir.join("memory.max"))
        .expect("read XP memory.max")
        .trim()
        .to_owned();
    let memory_swap_max = fs::read_to_string(cgroup_dir.join("memory.swap.max"))
        .expect("read XP memory.swap.max")
        .trim()
        .to_owned();
    assert_eq!(
        memory_max, "134217728",
        "XP workload must use a 128 MiB cgroup"
    );
    assert_eq!(
        memory_swap_max, "0",
        "XP workload must disable swap in its cgroup"
    );
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
    assert_expected_memory_scope(pid);
    let cpu_started = read_cpu_ticks(pid);
    let mut xp_peak_pss_kib = 0;
    let mut xp_peak_anon_pss_kib = 0;
    let mut xp_peak_file_pss_kib = 0;
    let mut stack_peak_pss_kib = 0;
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("poll XP workload") {
            let log = fs::read_to_string(&log_path).unwrap_or_default();
            panic!("XP exited during {label} workload with {status}:\n{log}");
        }
        let xp_pss = read_pss(pid).expect("read XP PSS");
        let support_pss = support_pids
            .iter()
            .map(|pid| {
                read_pss(*pid)
                    .unwrap_or_else(|| panic!("read support process {pid} PSS"))
                    .total_kib
            })
            .sum::<u64>();
        xp_peak_pss_kib = xp_peak_pss_kib.max(xp_pss.total_kib);
        xp_peak_anon_pss_kib = xp_peak_anon_pss_kib.max(xp_pss.anon_kib);
        xp_peak_file_pss_kib = xp_peak_file_pss_kib.max(xp_pss.file_kib);
        stack_peak_pss_kib = stack_peak_pss_kib.max(xp_pss.total_kib.saturating_add(support_pss));
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
        xp_peak_anon_pss_kib,
        xp_peak_file_pss_kib,
        stack_peak_pss_kib,
        cpu_ticks,
        tls_accepts,
        non_h2_requests,
        requests_per_peer,
        active_per_peer,
        peak_active_per_peer,
    }
}
