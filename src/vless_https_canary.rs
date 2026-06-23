use std::{
    collections::HashMap,
    fs, io,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context;
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{
        HeaderMap, HeaderValue, Method, Request, Response, StatusCode, Uri,
        header::{CONNECTION, HOST, UPGRADE},
    },
    response::IntoResponse,
    routing::get,
};
use chrono::{DateTime, NaiveDateTime, Utc};
use futures_util::TryStreamExt;
use hyper::upgrade;
use hyper_util::rt::TokioIo;
use lers::{
    Account, Certificate, Directory, Error as LersError, LETS_ENCRYPT_PRODUCTION_URL,
    solver::Solver,
};
use openssl::{
    pkey::{PKey, Private},
    x509::X509,
};
use serde::{Deserialize, Serialize};
use trust_dns_resolver::{
    TokioAsyncResolver,
    config::{NameServerConfigGroup, ResolverConfig, ResolverOpts},
};

use crate::{
    config::Config,
    domain::{Endpoint, EndpointKind},
    managed_default_endpoints::managed_default_vless_endpoint,
    ops::cloudflare,
    protocol::{CanaryUpstreamConfig, CanaryUpstreamMode},
    state::JsonSnapshotStore,
};

pub const GENERATE_204_PATH: &str = "/generate_204";
const READY_ATTEMPTS: usize = 60;
const READY_DELAY: Duration = Duration::from_secs(1);
const DNS_PROPAGATION_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthoritativeNameserver {
    host: String,
    ips: Vec<IpAddr>,
}

struct PreparedCanaryRuntime {
    paths: VlessHttpsCanaryPaths,
    rustls: axum_server::tls_rustls::RustlsConfig,
    listener: std::net::TcpListener,
}

#[derive(Clone)]
struct CanaryProxyState {
    store: Arc<tokio::sync::Mutex<JsonSnapshotStore>>,
    node_id: String,
    clients: Arc<CanaryProxyClients>,
}

struct CanaryProxyClients {
    auto: reqwest::Client,
    http1: reqwest::Client,
    h2c: reqwest::Client,
}

impl CanaryProxyClients {
    fn new() -> anyhow::Result<Self> {
        Ok(Self {
            auto: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(PROXY_CONNECT_TIMEOUT)
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .context("build canary auto upstream client")?,
            http1: reqwest::Client::builder()
                .http1_only()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(PROXY_CONNECT_TIMEOUT)
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .context("build canary http1 upstream client")?,
            h2c: reqwest::Client::builder()
                .http2_prior_knowledge()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(PROXY_CONNECT_TIMEOUT)
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .context("build canary h2c upstream client")?,
        })
    }

    fn for_mode(&self, mode: CanaryUpstreamMode) -> &reqwest::Client {
        match mode {
            CanaryUpstreamMode::Auto => &self.auto,
            CanaryUpstreamMode::Http1 => &self.http1,
            CanaryUpstreamMode::H2c => &self.h2c,
        }
    }
}

#[derive(Clone)]
struct RoutedUpstream {
    endpoint_id: String,
    upstream: CanaryUpstreamConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VlessHttpsCanaryStatus {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acme_directory_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_not_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_renewed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl VlessHttpsCanaryStatus {
    pub fn disabled(bind: std::net::SocketAddr) -> Self {
        Self {
            enabled: false,
            bind: Some(bind.to_string()),
            acme_directory_url: None,
            cert_not_after: None,
            last_renewed_at: None,
            last_error: None,
        }
    }
}

pub struct VlessHttpsCanaryPaths {
    pub dir: PathBuf,
    pub status_json: PathBuf,
    pub account_key_pem: PathBuf,
    pub cert_pem: PathBuf,
    pub key_pem: PathBuf,
}

impl VlessHttpsCanaryPaths {
    pub fn new(data_dir: &Path) -> Self {
        let dir = data_dir.join("vless-https-canary");
        Self {
            status_json: dir.join("status.json"),
            account_key_pem: dir.join("account_key.pem"),
            cert_pem: dir.join("cert.pem"),
            key_pem: dir.join("key.pem"),
            dir,
        }
    }
}

pub fn read_cloudflare_token_from_file(path: &Path) -> anyhow::Result<String> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read cloudflare token file {}", path.display()))?;
    let token = raw.trim();
    if token.is_empty() {
        anyhow::bail!("cloudflare token file is empty");
    }
    Ok(token.to_string())
}

pub async fn resolve_zone_id_for_host(
    api_base: &str,
    token: &str,
    configured_zone_id: &str,
    hostname: &str,
) -> anyhow::Result<String> {
    if !configured_zone_id.trim().is_empty() {
        return Ok(configured_zone_id.trim().to_string());
    }

    let candidates = zone_name_candidates(hostname);
    if candidates.is_empty() {
        anyhow::bail!("vless https canary hostname is empty");
    }
    for candidate in candidates {
        let mut zones = cloudflare::find_zone_by_name(api_base, token, &candidate)
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
        if zones.is_empty() {
            continue;
        }
        if zones.len() == 1 {
            return Ok(zones.remove(0).id);
        }
        anyhow::bail!(
            "multiple Cloudflare zones matched {candidate}; set XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID explicitly"
        );
    }

    anyhow::bail!("no Cloudflare zone matched vless https canary hostname {hostname}")
}

fn zone_name_candidates(domain: &str) -> Vec<String> {
    let trimmed = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let parts: Vec<&str> = trimmed.split('.').filter(|p| !p.is_empty()).collect();
    let mut out = Vec::new();
    for i in 0..parts.len() {
        out.push(parts[i..].join("."));
    }
    out
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, bytes)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn best_effort_chmod_0600(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

pub fn load_status(data_dir: &Path, bind: std::net::SocketAddr) -> VlessHttpsCanaryStatus {
    let paths = VlessHttpsCanaryPaths::new(data_dir);
    let Ok(raw) = fs::read(&paths.status_json) else {
        return VlessHttpsCanaryStatus::disabled(bind);
    };
    serde_json::from_slice(&raw).unwrap_or_else(|_| VlessHttpsCanaryStatus::disabled(bind))
}

pub fn persist_status(data_dir: &Path, status: &VlessHttpsCanaryStatus) -> anyhow::Result<()> {
    let paths = VlessHttpsCanaryPaths::new(data_dir);
    fs::create_dir_all(&paths.dir)
        .with_context(|| format!("create vless https canary dir {}", paths.dir.display()))?;
    let raw = serde_json::to_vec_pretty(status).context("serialize vless https canary status")?;
    write_atomic(&paths.status_json, &raw).with_context(|| {
        format!(
            "write vless https canary status {}",
            paths.status_json.display()
        )
    })?;
    Ok(())
}

pub fn persist_disabled_status(
    data_dir: &Path,
    bind: std::net::SocketAddr,
) -> anyhow::Result<()> {
    persist_status(data_dir, &VlessHttpsCanaryStatus::disabled(bind))
}

pub fn persist_disabled_status_with_error(
    data_dir: &Path,
    bind: std::net::SocketAddr,
    error: impl ToString,
) -> anyhow::Result<()> {
    let mut status = VlessHttpsCanaryStatus::disabled(bind);
    status.last_error = Some(error.to_string());
    persist_status(data_dir, &status)
}

pub fn ready_for_managed_vless(data_dir: &Path, bind: std::net::SocketAddr) -> bool {
    let status = load_status(data_dir, bind);
    status.enabled
        && status.bind.as_deref() == Some(bind.to_string().as_str())
        && status.last_error.is_none()
        && status.cert_not_after.is_some()
}

#[derive(Clone)]
struct RepoCloudflareDns01Solver {
    api_base: String,
    token: String,
    zone_id: String,
    client: reqwest::Client,
    records: Arc<Mutex<HashMap<String, String>>>,
    propagation_timeout: Duration,
}

impl RepoCloudflareDns01Solver {
    fn new(
        api_base: String,
        token: String,
        zone_id: String,
        propagation_timeout: Duration,
    ) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent("xp-vless-https-canary")
            .build()
            .context("build cloudflare dns01 client")?;
        Ok(Self {
            api_base,
            token,
            zone_id,
            client,
            records: Arc::new(Mutex::new(HashMap::new())),
            propagation_timeout,
        })
    }

    async fn create_txt_record(&self, fqdn: &str, content: &str) -> anyhow::Result<String> {
        let url = format!(
            "{}/client/v4/zones/{}/dns_records",
            self.api_base.trim_end_matches('/'),
            self.zone_id
        );
        let body = serde_json::json!({
            "type": "TXT",
            "name": fqdn,
            "content": content,
            "ttl": 120
        });
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        let value: serde_json::Value = resp.json().await?;
        let ok = value
            .get("success")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !ok {
            anyhow::bail!("cloudflare create txt record failed: {value}");
        }
        value
            .get("result")
            .and_then(|v| v.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| anyhow::anyhow!("cloudflare create txt record missing id"))
    }

    async fn wait_until_txt_visible(&self, fqdn: &str, content: &str) -> anyhow::Result<()> {
        let nameservers = authoritative_nameservers_for_fqdn(fqdn).await?;
        if nameservers.is_empty() {
            anyhow::bail!("no authoritative nameservers discovered for {fqdn}");
        }

        let deadline = tokio::time::Instant::now() + self.propagation_timeout;
        let fqdn = ensure_fqdn(fqdn);
        loop {
            let mut all_visible = true;
            for nameserver in &nameservers {
                if !authoritative_txt_contains_any_ip(nameserver, &fqdn, content).await? {
                    all_visible = false;
                    break;
                }
            }
            if all_visible {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "cloudflare TXT record {fqdn} did not become visible on authoritative nameservers within {}s",
                    self.propagation_timeout.as_secs()
                );
            }
            tokio::time::sleep(DNS_PROPAGATION_POLL_INTERVAL).await;
        }
    }
}

#[derive(Debug)]
struct VlessHttpsCanaryDnsError(String);

impl std::fmt::Display for VlessHttpsCanaryDnsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for VlessHttpsCanaryDnsError {}

#[async_trait::async_trait]
impl Solver for RepoCloudflareDns01Solver {
    async fn present(
        &self,
        domain: String,
        token: String,
        key_authorization: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        let domain = domain.trim_start_matches("*.").to_string();
        let fqdn = format!("_acme-challenge.{domain}");
        let record_id = self
            .create_txt_record(&fqdn, &key_authorization)
            .await
            .map_err(|err| lers::solver::boxed_err(VlessHttpsCanaryDnsError(err.to_string())))?;
        self.records
            .lock()
            .expect("dns01 record lock")
            .insert(token.clone(), record_id.clone());
        if let Err(err) = self.wait_until_txt_visible(&fqdn, &key_authorization).await {
            self.records.lock().expect("dns01 record lock").remove(&token);
            let client =
                cloudflare::CloudflareClient::new(self.api_base.clone(), self.token.clone());
            let cleanup_error = client.delete_dns_record(&self.zone_id, &record_id).await;
            let detail = match cleanup_error {
                Ok(()) => err.to_string(),
                Err(cleanup_err) => format!("{err}; cleanup failed: {cleanup_err}"),
            };
            return Err(lers::solver::boxed_err(VlessHttpsCanaryDnsError(detail)));
        }
        Ok(())
    }

    async fn cleanup(
        &self,
        token: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        let Some(record_id) = self.records.lock().expect("dns01 record lock").remove(token) else {
            return Ok(());
        };
        let client = cloudflare::CloudflareClient::new(self.api_base.clone(), self.token.clone());
        client
            .delete_dns_record(&self.zone_id, &record_id)
            .await
            .map_err(|err| lers::solver::boxed_err(VlessHttpsCanaryDnsError(err.to_string())))?;
        Ok(())
    }

    fn attempts(&self) -> usize {
        60
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(2)
    }
}

pub async fn spawn(
    config: Arc<Config>,
    store: Arc<tokio::sync::Mutex<JsonSnapshotStore>>,
    node_id: String,
) -> anyhow::Result<Option<std::thread::JoinHandle<()>>> {
    let prepared = match prepare_runtime(config.as_ref()).await {
        Ok(prepared) => prepared,
        Err(err) => {
            let _ = persist_disabled_status_with_error(
                &config.data_dir,
                config.vless_canary_bind,
                err.to_string(),
            );
            return Err(err);
        }
    };

    let Some(prepared) = prepared else {
        return Ok(None);
    };

    let bind = prepared.listener.local_addr().unwrap_or(config.vless_canary_bind);
    let config_for_thread = config.clone();
    let proxy_state = CanaryProxyState {
        store,
        node_id,
        clients: Arc::new(CanaryProxyClients::new()?),
    };
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build vless https canary runtime");
        runtime.block_on(async move {
            if let Err(err) =
                run_supervisor(config_for_thread.clone(), prepared, proxy_state).await
            {
                let mut status = base_status(&config_for_thread);
                status.last_error = Some(err.to_string());
                let _ = persist_status(&config_for_thread.data_dir, &status);
                tracing::error!(error = %err, "vless https canary supervisor failed");
            }
        });
    });
    wait_until_ready(&config.access_host, bind, READY_ATTEMPTS, READY_DELAY).await?;
    Ok(Some(handle))
}

async fn prepare_runtime(config: &Config) -> anyhow::Result<Option<PreparedCanaryRuntime>> {
    let mut status = base_status(config);
    persist_status(&config.data_dir, &status)?;

    if config.access_host.trim().is_empty() {
        return Ok(None);
    }

    let paths = VlessHttpsCanaryPaths::new(&config.data_dir);
    fs::create_dir_all(&paths.dir)
        .with_context(|| format!("create vless https canary dir {}", paths.dir.display()))?;

    let cert = ensure_certificate(config, &paths, &mut status).await?;
    let rustls = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert.fullchain_to_pem()?,
        cert.private_key_to_pem()?,
    )
    .await
    .context("build rustls config from vless https canary cert")?;

    persist_status(&config.data_dir, &status)?;

    let listener = std::net::TcpListener::bind(config.vless_canary_bind)
        .with_context(|| format!("bind vless https canary listener {}", config.vless_canary_bind))?;
    listener
        .set_nonblocking(true)
        .with_context(|| format!("set vless https canary listener nonblocking {}", config.vless_canary_bind))?;

    Ok(Some(PreparedCanaryRuntime {
        paths,
        rustls,
        listener,
    }))
}

pub async fn wait_until_ready(
    access_host: &str,
    bind: std::net::SocketAddr,
    attempts: usize,
    delay: Duration,
) -> anyhow::Result<()> {
    let host = access_host.trim().to_string();
    if host.is_empty() {
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .resolve(&host, bind)
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(2))
        .build()
        .context("build vless https canary readiness client")?;
    let url = format!("https://{host}{GENERATE_204_PATH}");
    let mut last_error = None;
    for _ in 0..attempts {
        if let Ok(resp) = client.get(&url).send().await
            && resp.status() == StatusCode::NO_CONTENT
        {
            return Ok(());
        }
        last_error = Some(format!("readiness probe did not return 204 for {url} via {bind}"));
        tokio::time::sleep(delay).await;
    }
    Err(anyhow::anyhow!(
        "{}",
        last_error.unwrap_or_else(|| {
            format!("vless https canary readiness probe timed out for {url} via {bind}")
        })
    ))
}

async fn run_supervisor(
    config: Arc<Config>,
    prepared: PreparedCanaryRuntime,
    proxy_state: CanaryProxyState,
) -> anyhow::Result<()> {
    let PreparedCanaryRuntime {
        paths,
        rustls,
        listener,
    } = prepared;

    let app = Router::new()
        .route(GENERATE_204_PATH, get(generate_204).head(generate_204))
        .fallback(canary_proxy)
        .with_state(proxy_state);
    let bind = config.vless_canary_bind;
    let rustls_reload = rustls.clone();
    let data_dir = config.data_dir.clone();
    let server = axum_server::from_tcp_rustls(listener, rustls)
        .context("build vless https canary rustls server")?;
    tokio::spawn(async move {
        if let Err(err) = server.serve(app.into_make_service()).await {
            let mut status = load_status(&data_dir, bind);
            status.last_error = Some(err.to_string());
            let _ = persist_status(&data_dir, &status);
        }
    });

    loop {
        let current = load_existing_certificate(&paths).ok();
        let Some(current) = current else {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        };
        let renew_after = renewal_sleep_duration(&current)?;
        tokio::time::sleep(renew_after).await;
        match renew_certificate(&config, &paths, current).await {
            Ok(cert) => {
                rustls_reload
                    .reload_from_pem(cert.fullchain_to_pem()?, cert.private_key_to_pem()?)
                    .await
                    .context("reload vless https canary rustls config")?;
                let mut status = base_status(&config);
                status.cert_not_after = certificate_not_after_rfc3339(cert.x509())?;
                status.last_renewed_at = Some(Utc::now().to_rfc3339());
                status.last_error = None;
                persist_status(&config.data_dir, &status)?;
            }
            Err(err) => {
                let mut status = load_status(&config.data_dir, config.vless_canary_bind);
                status.last_error = Some(err.to_string());
                persist_status(&config.data_dir, &status)?;
                tokio::time::sleep(Duration::from_secs(300)).await;
            }
        }
    }
}

fn base_status(config: &Config) -> VlessHttpsCanaryStatus {
    let enabled = !config.access_host.trim().is_empty();
    VlessHttpsCanaryStatus {
        enabled,
        bind: Some(config.vless_canary_bind.to_string()),
        acme_directory_url: enabled.then(|| config.vless_canary_acme_directory_url.clone()),
        cert_not_after: None,
        last_renewed_at: None,
        last_error: None,
    }
}

async fn generate_204() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

async fn canary_proxy(
    State(state): State<CanaryProxyState>,
    mut req: Request<Body>,
) -> impl IntoResponse {
    match proxy_request(&state, &mut req).await {
        Ok(resp) => resp,
        Err(resp) => resp,
    }
}

async fn proxy_request(
    state: &CanaryProxyState,
    req: &mut Request<Body>,
) -> Result<Response<Body>, Response<Body>> {
    let routed = route_upstream(state, req.headers(), req.uri()).await?;
    let upstream_url = build_upstream_url(&routed.upstream.url, req.uri()).map_err(error_response)?;
    if is_upgrade_request(req.headers()) {
        return proxy_websocket(state, req, routed, upstream_url).await;
    }
    proxy_http(state, req, routed, upstream_url).await
}

async fn route_upstream(
    state: &CanaryProxyState,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<RoutedUpstream, Response<Body>> {
    let authority = request_authority(headers, uri).ok_or_else(|| {
        text_response(
            StatusCode::MISDIRECTED_REQUEST,
            "missing Host or :authority for canary routing",
        )
    })?;
    let (host, port) = normalize_authority(&authority).map_err(|err| {
        text_response(
            StatusCode::MISDIRECTED_REQUEST,
            format!("invalid authority for canary routing: {err}"),
        )
    })?;

    let matches = {
        let store = state.store.lock().await;
        let Some(node) = store.get_node(&state.node_id) else {
            return Err(text_response(
                StatusCode::NOT_FOUND,
                format!("local node not found: {}", state.node_id),
            ));
        };
        if node.access_host.trim().trim_end_matches('.').to_ascii_lowercase() != host {
            Vec::new()
        } else {
            store
                .list_endpoints()
                .into_iter()
                .filter(|endpoint| endpoint.node_id == state.node_id)
                .filter_map(|endpoint| matching_managed_vless_endpoint(endpoint, port))
                .collect::<Vec<_>>()
        }
    };

    match matches.as_slice() {
        [] => Err(text_response(
            StatusCode::NOT_FOUND,
            format!("no managed VLESS endpoint matched authority {authority}"),
        )),
        [routed] if routed.upstream.url.trim().is_empty() => Err(text_response(
            StatusCode::NOT_FOUND,
            format!(
                "managed VLESS endpoint {} has no canary upstream configured",
                routed.endpoint_id
            ),
        )),
        [routed] => Ok(routed.clone()),
        _ => Err(text_response(
            StatusCode::CONFLICT,
            format!("multiple managed VLESS endpoints matched authority {authority}"),
        )),
    }
}

fn matching_managed_vless_endpoint(endpoint: Endpoint, port: u16) -> Option<RoutedUpstream> {
    if endpoint.kind != EndpointKind::VlessRealityVisionTcp || endpoint.port != port {
        return None;
    }
    let meta = managed_default_vless_endpoint(&endpoint)?;
    let upstream = meta.canary_upstream.unwrap_or(CanaryUpstreamConfig {
        url: String::new(),
        mode: CanaryUpstreamMode::Auto,
    });
    Some(RoutedUpstream {
        endpoint_id: endpoint.endpoint_id,
        upstream,
    })
}

async fn proxy_http(
    state: &CanaryProxyState,
    req: &mut Request<Body>,
    routed: RoutedUpstream,
    upstream_url: reqwest::Url,
) -> Result<Response<Body>, Response<Body>> {
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = std::mem::replace(req.body_mut(), Body::empty());
    let response = send_upstream_request(
        state.clients.for_mode(routed.upstream.mode),
        method,
        upstream_url,
        &headers,
        body,
        false,
    )
    .await
    .map_err(|err| upstream_error_response(&routed.endpoint_id, err))?;
    Ok(upstream_response_to_axum(response))
}

async fn proxy_websocket(
    state: &CanaryProxyState,
    req: &mut Request<Body>,
    routed: RoutedUpstream,
    upstream_url: reqwest::Url,
) -> Result<Response<Body>, Response<Body>> {
    let client_upgrade = upgrade::on(&mut *req);
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = std::mem::replace(req.body_mut(), Body::empty());
    let response = send_upstream_request(
        state.clients.for_mode(routed.upstream.mode),
        method,
        upstream_url,
        &headers,
        body,
        true,
    )
    .await
    .map_err(|err| upstream_error_response(&routed.endpoint_id, err))?;

    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        return Ok(upstream_response_to_axum(response));
    }

    let status = response.status();
    let headers = response.headers().clone();
    let upstream_upgrade = response.upgrade();
    tokio::spawn(async move {
        let (downstream, upstream) = tokio::join!(client_upgrade, upstream_upgrade);
        match (downstream, upstream) {
            (Ok(downstream), Ok(mut upstream)) => {
                let mut downstream = TokioIo::new(downstream);
                let _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await;
            }
            (Err(err), _) => {
                tracing::debug!(error = %err, "canary downstream websocket upgrade failed");
            }
            (_, Err(err)) => {
                tracing::debug!(error = %err, "canary upstream websocket upgrade failed");
            }
        }
    });

    let mut builder = Response::builder().status(status);
    for (name, value) in headers.iter() {
        if response_header_allowed(name.as_str(), true) {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Body::empty())
        .map_err(|err| error_response(anyhow::anyhow!(err)))
}

async fn send_upstream_request(
    client: &reqwest::Client,
    method: Method,
    url: reqwest::Url,
    headers: &HeaderMap,
    body: Body,
    upgrade: bool,
) -> reqwest::Result<reqwest::Response> {
    let mut request = client.request(method, url);
    for (name, value) in headers.iter() {
        if request_header_allowed(name.as_str(), upgrade) {
            request = request.header(name, value);
        }
    }
    request
        .body(reqwest::Body::wrap_stream(
            body.into_data_stream()
                .map_err(io::Error::other),
        ))
        .send()
        .await
}

fn upstream_response_to_axum(response: reqwest::Response) -> Response<Body> {
    let status = response.status();
    let headers = response.headers().clone();
    let stream = response
        .bytes_stream()
        .map_err(io::Error::other);
    let mut builder = Response::builder().status(status);
    for (name, value) in headers.iter() {
        if response_header_allowed(name.as_str(), false) {
            builder = builder.header(name, value);
        }
    }
    builder.body(Body::from_stream(stream)).unwrap_or_else(|err| {
        text_response(
            StatusCode::BAD_GATEWAY,
            format!("failed to build upstream response: {err}"),
        )
    })
}

fn request_authority(headers: &HeaderMap, uri: &Uri) -> Option<String> {
    uri.authority()
        .map(|authority| authority.as_str().to_string())
        .or_else(|| {
            headers
                .get(HOST)
                .and_then(|value| value.to_str().ok())
                .map(ToString::to_string)
        })
}

fn normalize_authority(authority: &str) -> anyhow::Result<(String, u16)> {
    let authority = authority.trim();
    if authority.is_empty() {
        anyhow::bail!("authority is empty");
    }
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((host, rest)) = bracketed.split_once(']') else {
            anyhow::bail!("invalid IPv6 authority");
        };
        let port = rest
            .strip_prefix(':')
            .map(str::parse::<u16>)
            .transpose()?
            .unwrap_or(443);
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') {
            (authority, 443)
        } else {
            (host, port.parse::<u16>()?)
        }
    } else {
        (authority, 443)
    };
    Ok((host.trim_end_matches('.').to_ascii_lowercase(), port))
}

fn build_upstream_url(base: &str, incoming: &Uri) -> anyhow::Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(base.trim()).context("parse endpoint canary upstream URL")?;
    let path_and_query = incoming
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let (path, query) = path_and_query
        .split_once('?')
        .map(|(path, query)| (path, Some(query)))
        .unwrap_or((path_and_query, None));
    url.set_path(path);
    url.set_query(query);
    Ok(url)
}

fn is_upgrade_request(headers: &HeaderMap) -> bool {
    let has_upgrade = headers
        .get(UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let connection_upgrade = headers
        .get(CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        });
    has_upgrade && connection_upgrade
}

fn request_header_allowed(name: &str, upgrade: bool) -> bool {
    let name = name.to_ascii_lowercase();
    if upgrade && matches!(name.as_str(), "connection" | "upgrade") {
        return true;
    }
    !matches!(
        name.as_str(),
        "connection"
            | "host"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn response_header_allowed(name: &str, upgrade: bool) -> bool {
    let name = name.to_ascii_lowercase();
    if upgrade && matches!(name.as_str(), "connection" | "upgrade") {
        return true;
    }
    !matches!(
        name.as_str(),
        "connection" | "keep-alive" | "proxy-authenticate" | "proxy-authorization" | "te"
            | "trailer" | "transfer-encoding" | "upgrade"
    )
}

fn upstream_error_response(endpoint_id: &str, err: reqwest::Error) -> Response<Body> {
    text_response(
        StatusCode::BAD_GATEWAY,
        format!("canary upstream request failed for endpoint {endpoint_id}: {err}"),
    )
}

fn error_response(err: anyhow::Error) -> Response<Body> {
    text_response(StatusCode::BAD_GATEWAY, err.to_string())
}

fn text_response(status: StatusCode, text: impl Into<String>) -> Response<Body> {
    let body = text.into();
    Response::builder()
        .status(status)
        .header("content-type", HeaderValue::from_static("text/plain; charset=utf-8"))
        .body(Body::from(body))
        .expect("build text response")
}

async fn ensure_certificate(
    config: &Config,
    paths: &VlessHttpsCanaryPaths,
    status: &mut VlessHttpsCanaryStatus,
) -> anyhow::Result<Certificate> {
    if let Ok(cert) = load_existing_certificate(paths)
        && !certificate_needs_renewal(cert.x509())?
    {
        status.cert_not_after = certificate_not_after_rfc3339(cert.x509())?;
        status.last_error = None;
        return Ok(cert);
    }

    let cert = obtain_certificate(config, paths).await?;
    write_certificate(paths, &cert)?;
    status.cert_not_after = certificate_not_after_rfc3339(cert.x509())?;
    status.last_renewed_at = Some(Utc::now().to_rfc3339());
    status.last_error = None;
    Ok(cert)
}

async fn renew_certificate(
    config: &Config,
    paths: &VlessHttpsCanaryPaths,
    current: Certificate,
) -> anyhow::Result<Certificate> {
    if !certificate_needs_renewal(current.x509())? {
        return Ok(current);
    }
    let account = load_or_create_account(config, paths).await?;
    let renewed = account
        .renew_certificate(current)
        .await
        .map_err(map_lers_error)?;
    write_certificate(paths, &renewed)?;
    Ok(renewed)
}

async fn obtain_certificate(
    config: &Config,
    paths: &VlessHttpsCanaryPaths,
) -> anyhow::Result<Certificate> {
    let account = load_or_create_account(config, paths).await?;
    let host = config.access_host.trim();
    if host.is_empty() {
        anyhow::bail!("XP_ACCESS_HOST is empty while vless https canary is enabled");
    }
    let cert = account
        .certificate()
        .add_domain(host)
        .obtain()
        .await
        .map_err(map_lers_error)?;
    Ok(cert)
}

async fn load_or_create_account(
    config: &Config,
    paths: &VlessHttpsCanaryPaths,
) -> anyhow::Result<Account> {
    let token = read_cloudflare_token_from_file(Path::new(
        config.vless_canary_cloudflare_token_file.as_str(),
    ))?;
    let zone_id = resolve_zone_id_for_host(
        &cloudflare::cloudflare_api_base(),
        &token,
        effective_vless_canary_zone_id(config),
        &config.access_host,
    )
    .await?;
    let solver = RepoCloudflareDns01Solver::new(
        cloudflare::cloudflare_api_base(),
        token,
        zone_id,
        Duration::from_secs(config.vless_canary_dns_propagation_timeout_secs),
    )?;
    let directory_url = if config.vless_canary_acme_directory_url.trim().is_empty() {
        LETS_ENCRYPT_PRODUCTION_URL.to_string()
    } else {
        config.vless_canary_acme_directory_url.trim().to_string()
    };
    let directory = Directory::builder(directory_url)
        .dns01_solver(Box::new(solver))
        .build()
        .await
        .map_err(map_lers_error)?;
    let mut builder = directory.account().terms_of_service_agreed(true);
    let email = config.vless_canary_acme_contact_email.trim();
    if !email.is_empty() {
        builder = builder.contacts(vec![format!("mailto:{email}")]);
    }
    if paths.account_key_pem.exists() {
        let raw = fs::read(&paths.account_key_pem).with_context(|| {
            format!(
                "read vless https canary account key {}",
                paths.account_key_pem.display()
            )
        })?;
        let key = PKey::<Private>::private_key_from_pem(&raw)
            .context("parse vless https canary account key")?;
        let account = builder
            .private_key(key)
            .create_if_not_exists()
            .await
            .map_err(map_lers_error)?;
        return Ok(account);
    }

    let account = builder.create_if_not_exists().await.map_err(map_lers_error)?;
    let pem = account
        .private_key()
        .private_key_to_pem_pkcs8()
        .context("export vless https canary account key")?;
    write_atomic(&paths.account_key_pem, &pem).with_context(|| {
        format!(
            "write vless https canary account key {}",
            paths.account_key_pem.display()
        )
    })?;
    best_effort_chmod_0600(&paths.account_key_pem);
    Ok(account)
}

fn load_existing_certificate(paths: &VlessHttpsCanaryPaths) -> anyhow::Result<Certificate> {
    let cert_pem = fs::read(&paths.cert_pem)
        .with_context(|| format!("read vless https canary cert {}", paths.cert_pem.display()))?;
    let key_pem = fs::read(&paths.key_pem)
        .with_context(|| format!("read vless https canary key {}", paths.key_pem.display()))?;
    Certificate::from_chain_and_private_key(
        lers::Format::Pem(&cert_pem),
        lers::Format::Pem(&key_pem),
    )
    .map_err(map_lers_error)
}

fn write_certificate(paths: &VlessHttpsCanaryPaths, cert: &Certificate) -> anyhow::Result<()> {
    write_atomic(&paths.cert_pem, &cert.fullchain_to_pem()?).with_context(|| {
        format!(
            "write vless https canary cert {}",
            paths.cert_pem.display()
        )
    })?;
    write_atomic(&paths.key_pem, &cert.private_key_to_pem()?).with_context(|| {
        format!("write vless https canary key {}", paths.key_pem.display())
    })?;
    best_effort_chmod_0600(&paths.key_pem);
    Ok(())
}

fn certificate_not_after_rfc3339(cert: &X509) -> anyhow::Result<Option<String>> {
    let dt = certificate_not_after_utc(cert)?;
    Ok(Some(dt.with_timezone(&Utc).to_rfc3339()))
}

fn certificate_needs_renewal(cert: &X509) -> anyhow::Result<bool> {
    let dt = certificate_not_after_utc(cert)?;
    Ok(dt.with_timezone(&Utc) <= Utc::now() + chrono::Duration::days(30))
}

fn renewal_sleep_duration(cert: &Certificate) -> anyhow::Result<Duration> {
    let dt = certificate_not_after_utc(cert.x509())?.with_timezone(&Utc);
    let renew_at = dt - chrono::Duration::days(30);
    let now = Utc::now();
    if renew_at <= now {
        return Ok(Duration::from_secs(1));
    }
    let diff = renew_at - now;
    Ok(Duration::from_secs(diff.num_seconds().max(1) as u64))
}

fn parse_openssl_not_after(not_after: &str) -> anyhow::Result<DateTime<chrono::FixedOffset>> {
    let naive = NaiveDateTime::parse_from_str(not_after, "%b %e %H:%M:%S %Y GMT")
        .or_else(|_| NaiveDateTime::parse_from_str(not_after, "%b %d %H:%M:%S %Y GMT"))
        .with_context(|| format!("parse certificate notAfter {not_after}"))?;
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).fixed_offset())
}

fn certificate_not_after_utc(cert: &X509) -> anyhow::Result<DateTime<chrono::FixedOffset>> {
    parse_openssl_not_after(&cert.not_after().to_string())
}

fn map_lers_error(err: LersError) -> anyhow::Error {
    anyhow::anyhow!(err.to_string())
}

fn effective_vless_canary_zone_id(config: &Config) -> &str {
    let configured = config.vless_canary_cloudflare_zone_id.trim();
    if !configured.is_empty() {
        return configured;
    }
    config.cloudflare_ddns_zone_id.trim()
}

fn ensure_fqdn(name: &str) -> String {
    let trimmed = name.trim().trim_end_matches('.');
    format!("{trimmed}.")
}

async fn authoritative_nameservers_for_fqdn(
    fqdn: &str,
) -> anyhow::Result<Vec<AuthoritativeNameserver>> {
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::cloudflare(), ResolverOpts::default())
        .context("build public recursive resolver for canary NS discovery")?;
    for candidate in zone_name_candidates(fqdn) {
        if candidate.split('.').count() < 2 {
            continue;
        }
        let zone = ensure_fqdn(&candidate);
        let response = match resolver.ns_lookup(zone.clone()).await {
            Ok(response) => response,
            Err(_) => continue,
        };

        let mut nameservers = Vec::new();
        for record in response.iter() {
            let host = ensure_fqdn(&record.to_string());
            let lookup = resolver
                .lookup_ip(host.clone())
                .await
                .with_context(|| format!("lookup IP for authoritative nameserver {host}"))?;
            let mut ips = Vec::new();
            for ip in lookup.iter() {
                ips.push(ip);
            }
            ips.sort();
            ips.dedup();
            if !ips.is_empty() {
                nameservers.push(AuthoritativeNameserver { host, ips });
            }
        }
        nameservers.sort_by(|a, b| a.host.cmp(&b.host));
        nameservers.dedup_by(|a, b| a.host == b.host);
        if !nameservers.is_empty() {
            return Ok(nameservers);
        }
    }
    anyhow::bail!("could not discover authoritative nameservers for {fqdn}")
}

async fn authoritative_txt_contains_any_ip(
    nameserver: &AuthoritativeNameserver,
    fqdn: &str,
    expected: &str,
) -> anyhow::Result<bool> {
    let mut saw_reachable = false;
    for ip in &nameserver.ips {
        match authoritative_txt_contains(ip, fqdn, expected).await {
            Ok(true) => {
                saw_reachable = true;
            }
            Ok(false) => {
                // Require all reachable addresses behind the same authoritative nameserver host
                // to agree before telling ACME the TXT is ready; otherwise multi-IP/anycast NS
                // pools can yield false positives.
                return Ok(false);
            }
            Err(_) => {
                continue;
            }
        }
    }
    Ok(saw_reachable)
}

async fn authoritative_txt_contains(
    nameserver: &IpAddr,
    fqdn: &str,
    expected: &str,
) -> anyhow::Result<bool> {
    let config = ResolverConfig::from_parts(
        None,
        vec![],
        NameServerConfigGroup::from_ips_clear(&[*nameserver], 53, true),
    );
    let resolver = TokioAsyncResolver::tokio(config, ResolverOpts::default())
        .context("build authoritative TXT resolver")?;
    let lookup = match resolver.txt_lookup(fqdn).await {
        Ok(lookup) => lookup,
        Err(_) => return Ok(false),
    };
    for txt in lookup.iter() {
        for chunk in txt.txt_data() {
            if chunk.as_ref() == expected.as_bytes() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster_identity::generate_cluster_ca;
    use crate::config::{Config, DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE, XrayRestartMode};
    use axum::routing::get;
    use rcgen::{
        CertificateParams, DistinguishedName, DnType, Issuer, KeyPair, PKCS_ECDSA_P256_SHA256,
    };
    use rustls::crypto::aws_lc_rs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{net::SocketAddr, sync::Once};
    use time::OffsetDateTime;
    use tempfile::tempdir;

    static RUSTLS_PROVIDER: Once = Once::new();

    fn install_test_crypto_provider() {
        RUSTLS_PROVIDER.call_once(|| {
            let _ = aws_lc_rs::default_provider().install_default();
        });
    }

    fn test_config(data_dir: PathBuf) -> Config {
        Config {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
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
            admin_token_hash: "hash".to_string(),
            node_name: "node-1".to_string(),
            access_host: "example.com".to_string(),
            api_base_url: "https://127.0.0.1:62416".to_string(),
            vless_canary_bind: SocketAddr::from(([127, 0, 0, 1], 39043)),
            vless_canary_acme_directory_url: LETS_ENCRYPT_PRODUCTION_URL.to_string(),
            vless_canary_acme_contact_email: String::new(),
            vless_canary_cloudflare_token_file: DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE.to_string(),
            vless_canary_cloudflare_zone_id: String::new(),
            vless_canary_dns_propagation_timeout_secs: 180,
            default_vless_port: None,
            default_vless_server_names: None,
            default_vless_fingerprint: None,
            default_ss_port: None,
            mesh_proxy_url: None,
            cloudflare_ddns_enabled: false,
            cloudflare_ddns_token_file: DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE.to_string(),
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

    #[test]
    fn persist_disabled_status_with_error_records_error() {
        let tmp = tempdir().unwrap();
        let bind: std::net::SocketAddr = "127.0.0.1:39043".parse().unwrap();

        persist_disabled_status_with_error(tmp.path(), bind, "dns setup failed").unwrap();

        let status = load_status(tmp.path(), bind);
        assert!(!status.enabled);
        assert_eq!(status.bind.as_deref(), Some("127.0.0.1:39043"));
        assert_eq!(status.last_error.as_deref(), Some("dns setup failed"));
    }

    #[test]
    fn ready_for_managed_vless_rejects_status_for_different_bind() {
        let tmp = tempdir().unwrap();
        let expected_bind: std::net::SocketAddr = "127.0.0.1:39043".parse().unwrap();
        let stale_bind: std::net::SocketAddr = "127.0.0.1:49043".parse().unwrap();

        persist_status(
            tmp.path(),
            &VlessHttpsCanaryStatus {
                enabled: true,
                bind: Some(stale_bind.to_string()),
                acme_directory_url: Some(LETS_ENCRYPT_PRODUCTION_URL.to_string()),
                cert_not_after: Some("2030-01-01T00:00:00Z".to_string()),
                last_renewed_at: None,
                last_error: None,
            },
        )
        .unwrap();

        assert!(!ready_for_managed_vless(tmp.path(), expected_bind));
    }

    #[test]
    fn effective_zone_id_prefers_explicit_canary_zone() {
        let mut config = test_config(tempdir().unwrap().path().to_path_buf());
        config.cloudflare_ddns_zone_id = "ddns-zone".to_string();
        config.vless_canary_cloudflare_zone_id = "canary-zone".to_string();

        assert_eq!(effective_vless_canary_zone_id(&config), "canary-zone");
    }

    #[test]
    fn effective_zone_id_falls_back_to_ddns_zone() {
        let mut config = test_config(tempdir().unwrap().path().to_path_buf());
        config.cloudflare_ddns_zone_id = "ddns-zone".to_string();
        config.vless_canary_cloudflare_zone_id = String::new();

        assert_eq!(effective_vless_canary_zone_id(&config), "ddns-zone");
    }

    #[test]
    fn ensure_fqdn_appends_trailing_dot_once() {
        assert_eq!(ensure_fqdn("example.com"), "example.com.");
        assert_eq!(ensure_fqdn("example.com."), "example.com.");
    }

    #[test]
    fn zone_name_candidates_walks_toward_zone_apex() {
        assert_eq!(
            zone_name_candidates("_acme-challenge.foo.example.com."),
            vec![
                "_acme-challenge.foo.example.com".to_string(),
                "foo.example.com".to_string(),
                "example.com".to_string(),
                "com".to_string(),
            ]
        );
    }

    #[test]
    fn normalize_authority_defaults_tls_port_and_lowercases_host() {
        assert_eq!(
            normalize_authority("Tokyo.EXAMPLE.com").unwrap(),
            ("tokyo.example.com".to_string(), 443)
        );
        assert_eq!(
            normalize_authority("Tokyo.EXAMPLE.com:53844").unwrap(),
            ("tokyo.example.com".to_string(), 53844)
        );
    }

    #[test]
    fn build_upstream_url_preserves_incoming_path_and_query() {
        let incoming: Uri = "/api/items?cursor=abc&limit=20".parse().unwrap();
        let url = build_upstream_url("http://127.0.0.1:8080/base", &incoming).unwrap();
        assert_eq!(url.as_str(), "http://127.0.0.1:8080/api/items?cursor=abc&limit=20");
    }

    #[test]
    fn response_header_filter_preserves_websocket_handshake_headers_only_for_upgrade() {
        assert!(!response_header_allowed("connection", false));
        assert!(!response_header_allowed("upgrade", false));
        assert!(response_header_allowed("connection", true));
        assert!(response_header_allowed("upgrade", true));
    }

    #[tokio::test]
    async fn canary_proxy_client_does_not_follow_redirects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/private\r\nContent-Length: 0\r\n\r\n",
                )
                .await
                .unwrap();
        });

        let clients = CanaryProxyClients::new().unwrap();
        let url = reqwest::Url::parse(&format!("http://{addr}/redirect")).unwrap();
        let response = send_upstream_request(
            clients.for_mode(CanaryUpstreamMode::Auto),
            Method::GET,
            url,
            &HeaderMap::new(),
            Body::empty(),
            false,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(
            response.headers().get("location").unwrap(),
            "http://127.0.0.1:9/private"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn canary_proxy_client_allows_slow_streaming_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 16\r\n\r\n",
                )
                .await
                .unwrap();
            stream.write_all(b"data: 1\n\n").await.unwrap();
            tokio::time::sleep(Duration::from_millis(750)).await;
            stream.write_all(b"data: 2\n\n").await.unwrap();
        });

        let clients = CanaryProxyClients::new().unwrap();
        let url = reqwest::Url::parse(&format!("http://{addr}/events")).unwrap();
        let response = send_upstream_request(
            clients.for_mode(CanaryUpstreamMode::Auto),
            Method::GET,
            url,
            &HeaderMap::new(),
            Body::empty(),
            false,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.unwrap();
        assert!(body.contains("data: 1"));
        assert!(body.contains("data: 2"));
        server.await.unwrap();
    }

    #[test]
    fn managed_vless_matching_keeps_unconfigured_upstream_diagnostic() {
        let endpoint = Endpoint {
            endpoint_id: "ep1".to_string(),
            node_id: "n1".to_string(),
            tag: "vless-ep1".to_string(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 53844,
            meta: serde_json::json!({
                "reality": {
                    "dest": "127.0.0.1:39043",
                    "server_names": ["node.example.com"],
                    "server_names_source": "manual",
                    "fingerprint": "chrome"
                },
                "reality_keys": {
                    "private_key": "private",
                    "public_key": "public"
                },
                "short_ids": ["aaaaaaaaaaaaaaaa"],
                "active_short_id": "aaaaaaaaaaaaaaaa",
                "managed_default": true
            }),
        };

        let routed = matching_managed_vless_endpoint(endpoint, 53844).unwrap();
        assert_eq!(routed.endpoint_id, "ep1");
        assert!(routed.upstream.url.is_empty());
        assert_eq!(routed.upstream.mode, CanaryUpstreamMode::Auto);
    }

    #[test]
    fn managed_vless_matching_requires_managed_default_flag_and_port() {
        let mut endpoint = Endpoint {
            endpoint_id: "ep1".to_string(),
            node_id: "n1".to_string(),
            tag: "vless-ep1".to_string(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 53844,
            meta: serde_json::json!({
                "reality": {
                    "dest": "127.0.0.1:39043",
                    "server_names": ["node.example.com"],
                    "server_names_source": "manual",
                    "fingerprint": "chrome"
                },
                "reality_keys": {
                    "private_key": "private",
                    "public_key": "public"
                },
                "short_ids": ["aaaaaaaaaaaaaaaa"],
                "active_short_id": "aaaaaaaaaaaaaaaa",
                "canary_upstream": {
                    "url": "http://127.0.0.1:8080",
                    "mode": "h2c"
                },
                "managed_default": false
            }),
        };

        assert!(matching_managed_vless_endpoint(endpoint.clone(), 53844).is_none());
        endpoint.meta["managed_default"] = serde_json::Value::Bool(true);
        assert!(matching_managed_vless_endpoint(endpoint.clone(), 443).is_none());
        let routed = matching_managed_vless_endpoint(endpoint, 53844).unwrap();
        assert_eq!(routed.upstream.url, "http://127.0.0.1:8080");
        assert_eq!(routed.upstream.mode, CanaryUpstreamMode::H2c);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_key_material_is_chmodded_0600() {
        let tmp = tempdir().unwrap();
        let paths = VlessHttpsCanaryPaths::new(tmp.path());
        fs::create_dir_all(&paths.dir).unwrap();

        write_atomic(&paths.account_key_pem, b"account-key").unwrap();
        best_effort_chmod_0600(&paths.account_key_pem);
        write_atomic(&paths.key_pem, b"tls-key").unwrap();
        best_effort_chmod_0600(&paths.key_pem);

        let account_mode = fs::metadata(&paths.account_key_pem)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let key_mode = fs::metadata(&paths.key_pem).unwrap().permissions().mode() & 0o777;

        assert_eq!(account_mode, 0o600);
        assert_eq!(key_mode, 0o600);
    }

    #[tokio::test]
    async fn wait_until_ready_accepts_self_signed_canary_cert() {
        install_test_crypto_provider();

        let ca = generate_cluster_ca("cluster-1").unwrap();
        let ca_key = KeyPair::from_pem(&ca.key_pem).unwrap();
        let ca_cert = Issuer::from_ca_cert_pem(&ca.cert_pem, ca_key).unwrap();

        let mut params =
            CertificateParams::new(vec!["canary.example.com".to_string()]).unwrap();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "canary.example.com");
        params.distinguished_name = dn;
        let now = OffsetDateTime::now_utc();
        params.not_before = now - time::Duration::days(1);
        params.not_after = now + time::Duration::days(30);

        let cert_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let cert = params.signed_by(&cert_key, &ca_cert).unwrap();
        let cert_pem = cert.pem();
        let key_pem = cert_key.serialize_pem();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let bind = listener.local_addr().unwrap();
        let rustls = axum_server::tls_rustls::RustlsConfig::from_pem(
            cert_pem.into_bytes(),
            key_pem.into_bytes(),
        )
            .await
            .unwrap();

        let app = Router::new().route(
            GENERATE_204_PATH,
            get(|| async { StatusCode::NO_CONTENT.into_response() }),
        );
        let server = axum_server::from_tcp_rustls(listener, rustls)
            .unwrap()
            .serve(app.into_make_service());
        let handle = tokio::spawn(server.into_future());

        let result = wait_until_ready(
            "canary.example.com",
            bind,
            5,
            Duration::from_millis(100),
        )
        .await;

        handle.abort();

        assert!(result.is_ok(), "unexpected readiness error: {result:?}");
    }

    #[test]
    fn authoritative_txt_policy_requires_all_reachable_ips_to_match() {
        fn reduce(results: &[Result<bool, ()>]) -> bool {
            let mut saw_reachable = false;
            for result in results {
                match result {
                    Ok(true) => {
                        saw_reachable = true;
                    }
                    Ok(false) => {
                        return false;
                    }
                    Err(()) => continue,
                }
            }
            saw_reachable
        }

        assert!(!reduce(&[Ok(true), Ok(false)]));
        assert!(reduce(&[Ok(true), Err(())]));
        assert!(!reduce(&[Err(()), Err(())]));
    }

    #[test]
    fn parse_openssl_not_after_accepts_double_digit_day() {
        let parsed = parse_openssl_not_after("Sep 16 09:13:04 2026 GMT")
            .expect("double-digit day should parse");
        assert_eq!(parsed.to_rfc3339(), "2026-09-16T09:13:04+00:00");
    }
}
