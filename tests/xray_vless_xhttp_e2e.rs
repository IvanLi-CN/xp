use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{Response, StatusCode, Uri, header},
    routing::{any, get},
};
use futures_util::future::join_all;
use rand::rngs::OsRng;
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use serde_yaml::Value;
use tokio::{
    io::copy_bidirectional,
    net::TcpListener,
    process::{Child, Command},
    sync::broadcast,
    task::JoinHandle,
    time::{Instant, sleep},
};

use xp::{
    credentials,
    domain::{
        Endpoint, EndpointKind, Node, NodeQuotaReset, User, UserPriorityTier, UserQuotaReset,
    },
    protocol::{
        MihomoSmuxConfig, RealityConfig, RealityKeys, RealityServerNamesSource,
        VlessRealityTransport, VlessRealityVisionTcpEndpointMeta, generate_reality_keypair,
    },
    state::{NodeUserEndpointMembership, UserMihomoProfile, membership_xray_email},
    subscription, xray,
};

const RESPONSE_BODY: &str = "xp-xhttp-e2e-ok";

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
    disconnect: broadcast::Sender<()>,
    task: JoinHandle<()>,
}

impl CountingProxy {
    async fn disconnect_all(&self) {
        let _ = self.disconnect.send(());
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.active.load(Ordering::SeqCst) != 0 {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("proxied XHTTP connections close after forced disconnect");
    }
}

impl Drop for CountingProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct MihomoProcess {
    child: Option<Child>,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

impl MihomoProcess {
    async fn stop(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }

    fn logs(&self) -> String {
        format!(
            "stdout:\n{}\nstderr:\n{}",
            std::fs::read_to_string(&self.stdout_path).unwrap_or_default(),
            std::fs::read_to_string(&self.stderr_path).unwrap_or_default(),
        )
    }
}

impl Drop for MihomoProcess {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }
}

#[derive(Clone)]
struct MihomoProviderPayloads {
    system: String,
    external: String,
}

async fn mihomo_provider_payload(
    State(payloads): State<MihomoProviderPayloads>,
    uri: Uri,
) -> Response<Body> {
    let payload = if uri.path().ends_with("system.yaml") {
        payloads.system
    } else {
        payloads.external
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/yaml")
        .body(Body::from(payload))
        .expect("provider response")
}

async fn spawn_mihomo_provider_server(
    addr: SocketAddr,
    payloads: MihomoProviderPayloads,
) -> TestServer {
    let listener = TcpListener::bind(addr)
        .await
        .expect("Mihomo provider listener");
    let addr = listener.local_addr().expect("Mihomo provider address");
    let app = Router::new()
        .fallback(any(mihomo_provider_payload))
        .with_state(payloads);
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("Mihomo provider server");
    });
    TestServer { addr, task }
}

fn mihomo_controller_proxy_url(controller_addr: SocketAddr, proxy_name: &str) -> reqwest::Url {
    let mut url =
        reqwest::Url::parse(&format!("http://{controller_addr}")).expect("Mihomo controller URL");
    let mut segments = url
        .path_segments_mut()
        .expect("Mihomo controller URL supports path segments");
    segments.push("proxies");
    segments.push(proxy_name);
    drop(segments);
    url
}

async fn wait_for_mihomo_proxy_api<F>(
    client: &reqwest::Client,
    controller_addr: SocketAddr,
    proxy_name: &str,
    mihomo: &MihomoProcess,
    ready: F,
) -> serde_json::Value
where
    F: Fn(&serde_json::Value) -> bool,
{
    let url = mihomo_controller_proxy_url(controller_addr, proxy_name);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let last_error = match client.get(url.clone()).send().await {
            Ok(response) if response.status().is_success() => {
                match response.json::<serde_json::Value>().await {
                    Ok(value) if ready(&value) => return value,
                    Ok(value) => format!("unexpected proxy payload: {value}"),
                    Err(error) => format!("decode proxy payload: {error}"),
                }
            }
            Ok(response) => format!("controller status: {}", response.status()),
            Err(error) => format!("controller request: {error}"),
        };
        if Instant::now() >= deadline {
            panic!(
                "Mihomo proxy {proxy_name:?} did not become ready: {}; {}",
                last_error,
                mihomo.logs()
            );
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_mihomo_provider_yaml<F>(
    home: &Path,
    provider_name: &str,
    mihomo: &MihomoProcess,
    ready: F,
) -> Value
where
    F: Fn(&Value) -> bool,
{
    let path = home.join("providers").join(format!("{provider_name}.yaml"));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let last_error = match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_yaml::from_str::<Value>(&contents) {
                Ok(value) if ready(&value) => return value,
                Ok(value) => format!("unexpected provider payload: {value:?}"),
                Err(error) => format!("decode provider payload: {error}"),
            },
            Err(error) => format!("read provider cache: {error}"),
        };
        if Instant::now() >= deadline {
            panic!(
                "Mihomo provider {provider_name:?} did not become ready: {}; {}",
                last_error,
                mihomo.logs()
            );
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn spawn_plain_http_server() -> TestServer {
    async fn response() -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONNECTION, "close")
            .body(Body::from(RESPONSE_BODY))
            .expect("HTTP response")
    }

    let listener = TcpListener::bind("0.0.0.0:0").await.expect("HTTP listener");
    let addr = listener.local_addr().expect("HTTP listener address");
    let app = Router::new().route("/xhttp-e2e", get(response));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("HTTP test server");
    });
    TestServer { addr, task }
}

async fn spawn_reality_destination() -> TestServer {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("TLS key");
    let cert = CertificateParams::new(xp_test_fixtures::host_list_edge1())
        .expect("TLS certificate params")
        .self_signed(&key)
        .expect("self-signed TLS certificate");
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert.pem().into_bytes(),
        key.serialize_pem().into_bytes(),
    )
    .await
    .expect("TLS config");
    let listener = std::net::TcpListener::bind("0.0.0.0:0").expect("TLS listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking TLS listener");
    let addr = listener.local_addr().expect("TLS listener address");
    let app = Router::new().route("/", get(|| async { StatusCode::NO_CONTENT }));
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
    let (disconnect, _) = broadcast::channel(1);
    let accepts_for_task = accepts.clone();
    let active_for_task = active.clone();
    let disconnect_for_task = disconnect.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut downstream, _)) = listener.accept().await else {
                break;
            };
            accepts_for_task.fetch_add(1, Ordering::SeqCst);
            let active = active_for_task.clone();
            let mut disconnect = disconnect_for_task.subscribe();
            tokio::spawn(async move {
                let Ok(mut upstream) = tokio::net::TcpStream::connect(upstream).await else {
                    return;
                };
                active.fetch_add(1, Ordering::SeqCst);
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
        disconnect,
        task,
    }
}

async fn free_loopback_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral listener");
    listener.local_addr().expect("ephemeral address").port()
}

async fn wait_for_xray_inbound(addr: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(_) => return,
            Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(50)).await,
            Err(error) => panic!("Xray XHTTP inbound did not become ready: {error}"),
        }
    }
}

async fn spawn_mihomo(
    binary: &Path,
    home: &Path,
    config_yaml: &str,
    socks_port: u16,
) -> MihomoProcess {
    let version = Command::new(binary)
        .arg("-v")
        .output()
        .await
        .expect("run Mihomo version check");
    let version_text = format!(
        "{}{}",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
    assert!(
        version.status.success() && version_text.contains("v1.19.29"),
        "expected official Mihomo v1.19.29, got: {version_text}"
    );

    let config_path = home.join("config.yaml");
    let stdout_path = home.join("mihomo.stdout.log");
    let stderr_path = home.join("mihomo.stderr.log");
    std::fs::write(&config_path, config_yaml).expect("write ephemeral Mihomo config");
    let stdout = std::fs::File::create(&stdout_path).expect("create Mihomo stdout log");
    let stderr = std::fs::File::create(&stderr_path).expect("create Mihomo stderr log");
    let child = Command::new(binary)
        .arg("-d")
        .arg(home)
        .arg("-f")
        .arg(&config_path)
        .kill_on_drop(true)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn Mihomo");
    let mut process = MihomoProcess {
        child: Some(child),
        stdout_path,
        stderr_path,
    };

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", socks_port))
            .await
            .is_ok()
        {
            return process;
        }
        if let Some(status) = process
            .child
            .as_mut()
            .expect("Mihomo child")
            .try_wait()
            .expect("poll Mihomo")
        {
            panic!(
                "Mihomo exited during startup ({status}): {}",
                process.logs()
            );
        }
        assert!(
            Instant::now() < deadline,
            "Mihomo SOCKS listener did not become ready: {}",
            process.logs()
        );
        sleep(Duration::from_millis(50)).await;
    }
}

fn xhttp_endpoint(port: u16) -> Endpoint {
    let keypair = generate_reality_keypair(&mut OsRng);
    Endpoint {
        endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
        kind: EndpointKind::VlessRealityVisionTcp,
        port,
        meta: serde_json::to_value(VlessRealityVisionTcpEndpointMeta {
            reality: RealityConfig {
                dest: xp_test_fixtures::address_loopback_port39001().to_owned(),
                server_names: xp_test_fixtures::host_list_edge1(),
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
            accepted_authorities: xp_test_fixtures::host_list_empty(),
            mihomo_smux: MihomoSmuxConfig::default(),
            transport: VlessRealityTransport::Xhttp,
            managed_default: true,
        })
        .expect("serialize XHTTP endpoint metadata"),
    }
}

fn set_reality_destination_port(endpoint: &mut Endpoint, port: u16) {
    endpoint.meta["reality"]["dest"] =
        serde_json::Value::String(format!("host.docker.internal:{port}"));
}

fn render_mihomo_config(endpoint: &Endpoint, external_port: u16, socks_port: u16) -> String {
    let user = User {
        user_id: xp_test_fixtures::primary_user_id().to_owned(),
        display_name: "xhttp-e2e".to_string(),
        subscription_token: xp_test_fixtures::primary_token().to_owned(),
        credential_epoch: 0,
        priority_tier: UserPriorityTier::P2,
        quota_reset: UserQuotaReset::default(),
    };
    let node = Node {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        access_host: xp_test_fixtures::address_loopback().to_owned(),
        api_base_url: xp_test_fixtures::url_loopback1().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    let membership = NodeUserEndpointMembership {
        user_id: xp_test_fixtures::primary_user_id().to_owned(),
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
    };
    let mut advertised_endpoint = endpoint.clone();
    advertised_endpoint.port = external_port;
    let rendered = subscription::build_clash_yaml(
        xp_test_fixtures::primary_token(),
        &user,
        &[membership],
        &[advertised_endpoint],
        &[node],
    )
    .expect("render Mihomo XHTTP subscription");
    let mut root: serde_yaml::Mapping = serde_yaml::from_str(&rendered).expect("subscription YAML");
    let proxy_name = root
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| proxies.first())
        .and_then(|proxy| proxy.get("name"))
        .and_then(Value::as_str)
        .expect("generated proxy name")
        .to_string();
    root.insert("socks-port".into(), (socks_port as u64).into());
    root.insert("allow-lan".into(), false.into());
    root.insert("mode".into(), "rule".into());
    root.insert("log-level".into(), "warning".into());
    root.insert("ipv6".into(), false.into());
    root.insert("find-process-mode".into(), "off".into());
    root.insert(
        "rules".into(),
        Value::Sequence(vec![format!("MATCH,{proxy_name}").into()]),
    );
    serde_yaml::to_string(&root).expect("serialize Mihomo E2E config")
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<(), String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("request through Mihomo XHTTP proxy: {error}"))?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("unexpected response status: {}", response.status()));
    }
    let response_body = response
        .text()
        .await
        .map_err(|error| format!("read response body: {error}"))?;
    if response_body != RESPONSE_BODY {
        return Err(format!("unexpected response body: {response_body}"));
    }
    Ok(())
}

async fn wait_for_xhttp_client_ready(client: &reqwest::Client, url: &str, mihomo: &MihomoProcess) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_error = None;
    loop {
        match fetch(client, url).await {
            Ok(()) => return,
            Err(error) if Instant::now() < deadline => {
                last_error = Some(error);
                sleep(Duration::from_millis(50)).await;
            }
            Err(error) => {
                panic!(
                    "Mihomo XHTTP client did not become ready: {error}; previous error: {}; {}",
                    last_error.unwrap_or_default(),
                    mihomo.logs()
                );
            }
        }
    }
}

#[tokio::test]
#[ignore]
async fn mihomo_xhttp_xmux_reuses_one_connection_and_recovers_after_disconnect() {
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
    let mihomo_binary = PathBuf::from(
        std::env::var("XP_E2E_MIHOMO_BIN").expect("XP_E2E_MIHOMO_BIN from E2E harness"),
    );

    let reality_destination = spawn_reality_destination().await;
    let target = spawn_plain_http_server().await;
    let mut endpoint = xhttp_endpoint(vless_port);
    set_reality_destination_port(&mut endpoint, reality_destination.addr.port());
    let uuid = credentials::derive_vless_uuid(
        xp_test_fixtures::primary_token(),
        xp_test_fixtures::primary_user_id(),
        0,
    )
    .expect("derive VLESS UUID");
    let mut xray = xray::connect(xray_api_addr)
        .await
        .expect("connect Xray API");
    xray.add_inbound(xp::xray::builder::build_add_inbound_request(&endpoint).unwrap())
        .await
        .expect("add Xray XHTTP inbound");
    xray.alter_inbound(
        xp::xray::proto::xray::app::proxyman::command::AlterInboundRequest {
            tag: xp_test_fixtures::primary_endpoint_tag().to_owned(),
            operation: Some(
                xp::xray::builder::build_add_user_operation(
                    &endpoint,
                    &membership_xray_email(
                        xp_test_fixtures::primary_user_id(),
                        &endpoint.endpoint_id,
                    ),
                    Some(&uuid),
                    None,
                )
                .expect("build XHTTP user"),
            ),
        },
    )
    .await
    .expect("add XHTTP user");
    wait_for_xray_inbound(SocketAddr::from(([127, 0, 0, 1], vless_port))).await;

    let proxy = spawn_counting_proxy(SocketAddr::from(([127, 0, 0, 1], vless_port))).await;
    let socks_port = free_loopback_port().await;
    let mihomo_temp_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("xray-vless-xhttp-e2e");
    std::fs::create_dir_all(&mihomo_temp_root).expect("create Mihomo E2E temp root");
    let mihomo_home = tempfile::tempdir_in(mihomo_temp_root).expect("Mihomo temp directory");
    let config = render_mihomo_config(&endpoint, proxy.addr.port(), socks_port);
    let mihomo = spawn_mihomo(&mihomo_binary, mihomo_home.path(), &config, socks_port).await;
    let client = reqwest::Client::builder()
        .proxy(
            reqwest::Proxy::all(format!("socks5h://127.0.0.1:{socks_port}"))
                .expect("SOCKS proxy URL"),
        )
        .pool_max_idle_per_host(0)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("HTTP client through Mihomo");
    let target_url = format!(
        "http://host.docker.internal:{}/xhttp-e2e",
        target.addr.port()
    );

    wait_for_xhttp_client_ready(&client, &target_url, &mihomo).await;
    for _ in 0..32 {
        fetch(&client, &target_url)
            .await
            .expect("sequential request through Mihomo XHTTP proxy");
    }
    let concurrent = join_all((0..64).map(|_| fetch(&client, &target_url))).await;
    assert_eq!(concurrent.len(), 64);
    for result in concurrent {
        result.expect("concurrent request through Mihomo XHTTP proxy");
    }
    assert_eq!(
        proxy.accepts.load(Ordering::SeqCst),
        1,
        "warm-up plus 32 sequential and 64 concurrent proxy streams must share one external TCP"
    );
    assert_eq!(proxy.active.load(Ordering::SeqCst), 1);

    proxy.disconnect_all().await;
    fetch(&client, &target_url)
        .await
        .expect("request after forced XHTTP transport disconnect");
    assert_eq!(
        proxy.accepts.load(Ordering::SeqCst),
        2,
        "the first request after a forced transport cut must reconnect exactly once"
    );
    assert_eq!(proxy.active.load(Ordering::SeqCst), 1);

    mihomo.stop().await;
    xray.remove_inbound(
        xp::xray::proto::xray::app::proxyman::command::RemoveInboundRequest { tag: endpoint.tag },
    )
    .await
    .expect("remove Xray XHTTP inbound");
}

#[tokio::test]
#[ignore]
async fn mihomo_provider_chain_has_no_direct_fallback() {
    let mihomo_binary = std::env::var("XP_E2E_MIHOMO_BIN")
        .expect("XP_E2E_MIHOMO_BIN for the real Mihomo provider smoke");

    let user = User {
        user_id: xp_test_fixtures::primary_user_id().to_owned(),
        display_name: "mihomo-provider-e2e".to_string(),
        subscription_token: xp_test_fixtures::primary_token().to_owned(),
        credential_epoch: 0,
        priority_tier: UserPriorityTier::P2,
        quota_reset: UserQuotaReset::default(),
    };
    let node = Node {
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        node_name: xp_test_fixtures::primary_node_name().to_owned(),
        access_host: xp_test_fixtures::address_loopback().to_owned(),
        api_base_url: xp_test_fixtures::url_loopback1().to_owned(),
        quota_limit_bytes: 0,
        quota_reset: NodeQuotaReset::default(),
    };
    let endpoint = xhttp_endpoint(8443);
    let membership = NodeUserEndpointMembership {
        user_id: user.user_id.clone(),
        node_id: xp_test_fixtures::primary_node_id().to_owned(),
        endpoint_id: xp_test_fixtures::primary_endpoint_id().to_owned(),
    };
    let provider_port = free_loopback_port().await;
    let provider_addr = SocketAddr::from(([127, 0, 0, 1], provider_port));
    let system_provider_url = format!("http://{provider_addr}/system.yaml");
    let external_provider_url = format!("http://{provider_addr}/provider-a.yaml");
    let profile = UserMihomoProfile {
        mixin_yaml: "port: 0\nrules: []\n".to_string(),
        extra_proxies_yaml: String::new(),
        extra_proxy_providers_yaml: format!(
            "providerA:\n  type: http\n  path: ./providers/provider-a.yaml\n  url: {}\n",
            external_provider_url
        ),
    };
    let memberships = vec![membership.clone()];
    let endpoints = vec![endpoint.clone()];
    let nodes = vec![node.clone()];
    let system_yaml = subscription::build_mihomo_provider_system_yaml(
        "seed",
        &user,
        &memberships,
        &endpoints,
        &nodes,
    )
    .expect("render Mihomo system provider");
    let external_yaml = r#"
proxies:
  - name: "Germany smoke"
    type: http
    server: 127.0.0.1
    port: 1
"#
    .to_string();
    let rendered = subscription::build_mihomo_provider_yaml(
        "seed",
        &user,
        &memberships,
        &endpoints,
        &nodes,
        &profile,
        &system_provider_url,
    )
    .expect("render Mihomo provider config");
    let mut root: serde_yaml::Mapping = serde_yaml::from_str(&rendered).expect("provider YAML");
    let relay_group_name = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups.iter().find_map(|group| {
                group
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| name.starts_with("🛣️ "))
                    .map(str::to_owned)
            })
        })
        .expect("generated relay group");
    let relay_group = root
        .get("proxy-groups")
        .and_then(Value::as_sequence)
        .and_then(|groups| {
            groups.iter().find(|group| {
                group.get("name").and_then(Value::as_str) == Some(relay_group_name.as_str())
            })
        })
        .expect("generated relay group definition");
    assert!(
        relay_group.get("proxies").is_none(),
        "external relay groups must not contain a DIRECT fallback"
    );

    let system_root: Value = serde_yaml::from_str(&system_yaml).expect("system provider YAML");
    let direct_name = system_root
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies.iter().find_map(|proxy| {
                proxy
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| name.ends_with("-reality"))
                    .map(str::to_owned)
            })
        })
        .expect("direct Reality proxy");
    let chain_name = system_root
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies.iter().find_map(|proxy| {
                proxy
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| name.ends_with("-reality-chain"))
                    .map(str::to_owned)
            })
        })
        .expect("chain Reality proxy");
    assert!(!system_yaml.contains("dialer-proxy: DIRECT"));

    let socks_port = free_loopback_port().await;
    let controller_port = free_loopback_port().await;
    root.insert("socks-port".into(), (socks_port as u64).into());
    root.insert("allow-lan".into(), false.into());
    root.insert("mode".into(), "rule".into());
    root.insert("log-level".into(), "warning".into());
    root.insert("ipv6".into(), false.into());
    root.insert(
        "external-controller".into(),
        format!("127.0.0.1:{controller_port}").into(),
    );
    let config_yaml = serde_yaml::to_string(&root).expect("serialize Mihomo provider config");
    let _provider_server = spawn_mihomo_provider_server(
        provider_addr,
        MihomoProviderPayloads {
            system: system_yaml,
            external: external_yaml,
        },
    )
    .await;

    let mihomo_temp_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("mihomo-provider-e2e");
    std::fs::create_dir_all(&mihomo_temp_root).expect("create Mihomo provider temp root");
    let mihomo_home = tempfile::tempdir_in(mihomo_temp_root).expect("Mihomo provider temp dir");
    std::fs::create_dir_all(mihomo_home.path().join("providers"))
        .expect("create Mihomo provider directory");
    let mihomo = spawn_mihomo(
        Path::new(&mihomo_binary),
        mihomo_home.path(),
        &config_yaml,
        socks_port,
    )
    .await;
    let controller_addr = SocketAddr::from(([127, 0, 0, 1], controller_port));
    let client = reqwest::Client::new();
    let loaded_external =
        wait_for_mihomo_provider_yaml(mihomo_home.path(), "provider-a", &mihomo, |value| {
            value.get("proxies").and_then(Value::as_sequence).is_some()
        })
        .await;
    assert!(
        loaded_external
            .get("proxies")
            .and_then(Value::as_sequence)
            .is_some_and(|proxies| proxies.iter().any(|proxy| {
                proxy.get("name").and_then(Value::as_str) == Some("Germany smoke")
            }))
    );
    let relay = wait_for_mihomo_proxy_api(
        &client,
        controller_addr,
        &relay_group_name,
        &mihomo,
        |value| {
            value
                .get("all")
                .and_then(serde_json::Value::as_array)
                .is_some()
        },
    )
    .await;
    let relay_candidates = relay
        .get("all")
        .and_then(serde_json::Value::as_array)
        .expect("Mihomo relay candidates");
    assert!(
        relay_candidates.iter().all(|candidate| {
            matches!(candidate.as_str(), Some(name) if name != "DIRECT" && name != "Germany smoke")
        }),
        "Mihomo relay candidates must exclude DIRECT and non-matching provider candidates"
    );

    let loaded_system = wait_for_mihomo_provider_yaml(
        mihomo_home.path(),
        "xp-system-generated",
        &mihomo,
        |value| {
            value
                .get("proxies")
                .and_then(Value::as_sequence)
                .is_some_and(|proxies| {
                    proxies.iter().any(|proxy| {
                        proxy.get("name").and_then(Value::as_str) == Some(direct_name.as_str())
                    }) && proxies.iter().any(|proxy| {
                        proxy.get("name").and_then(Value::as_str) == Some(chain_name.as_str())
                    })
                })
        },
    )
    .await;
    let loaded_direct = loaded_system
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies.iter().find(|proxy| {
                proxy.get("name").and_then(Value::as_str) == Some(direct_name.as_str())
            })
        })
        .expect("loaded direct Reality proxy");
    assert!(loaded_direct.get("dialer-proxy").is_none());
    let loaded_chain = loaded_system
        .get("proxies")
        .and_then(Value::as_sequence)
        .and_then(|proxies| {
            proxies.iter().find(|proxy| {
                proxy.get("name").and_then(Value::as_str) == Some(chain_name.as_str())
            })
        })
        .expect("loaded chain Reality proxy");
    assert_eq!(
        loaded_chain.get("dialer-proxy").and_then(Value::as_str),
        Some(relay_group_name.as_str())
    );
    mihomo.stop().await;
}
