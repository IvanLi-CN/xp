use crate::admin_token::{hash_admin_token_argon2id, parse_admin_token_hash};
use crate::cluster_metadata::{ClusterMetadata, ClusterPaths};
use crate::domain::{Endpoint, EndpointKind};
use crate::id::new_ulid_string;
use crate::ops::cli::{CloudflareProvisionArgs, ContainerRunArgs, ExitError};
use crate::ops::cloudflare::{self, CloudflareTokenSource, ZoneLookup};
use crate::ops::init;
use crate::ops::paths::Paths;
use crate::ops::util::{Mode, ensure_dir, write_string_if_changed};
use crate::protocol::{
    RealityConfig, RealityKeys, RealityServerNamesSource, SS2022_METHOD_2022_BLAKE3_AES_128_GCM,
    Ss2022EndpointMeta, VlessRealityVisionTcpEndpointMeta, generate_reality_keypair,
    generate_short_id_16hex, generate_ss2022_psk_b64, validate_reality_server_name,
};
use futures_util::future::pending;
use rand::rngs::OsRng;
use reqwest::Url;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{Instant, sleep, timeout};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

const DEFAULT_BIND: &str = "0.0.0.0:62416";
const DEFAULT_XRAY_API_ADDR: &str = "127.0.0.1:10085";
const DEFAULT_DATA_DIR: &str = "/var/lib/xp/data";
const DEFAULT_CLOUDFLARE_ORIGIN_URL: &str = "http://127.0.0.1:62416";
const CHILD_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
const CLOUDFLARED_STARTUP_GRACE: Duration = Duration::from_secs(5);
const XRAY_READY_TIMEOUT: Duration = Duration::from_secs(20);
const LOCAL_API_READY_TIMEOUT: Duration = Duration::from_secs(20);
const LOCAL_API_READY_DELAY: Duration = Duration::from_millis(300);
const PUBLIC_API_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PUBLIC_API_PROBE_DELAY: Duration = Duration::from_secs(2);
const PUBLIC_API_PROBE_ATTEMPTS: usize = 60;
const MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
struct BinaryPaths {
    xp: PathBuf,
    xray: PathBuf,
    cloudflared: PathBuf,
}

#[derive(Debug, Clone)]
enum ContainerStartup {
    Bootstrap {
        needs_init: bool,
    },
    Join {
        join_token: String,
        needs_join: bool,
    },
    ReuseJoined,
}

impl ContainerStartup {
    fn requires_bootstrap_token(&self) -> bool {
        matches!(self, Self::Bootstrap { .. })
    }

    fn needs_join_wait(&self) -> bool {
        matches!(
            self,
            Self::Join {
                needs_join: true,
                ..
            }
        )
    }

    fn needs_init(&self) -> bool {
        matches!(self, Self::Bootstrap { needs_init: true })
    }

    fn needs_join(&self) -> bool {
        matches!(
            self,
            Self::Join {
                needs_join: true,
                ..
            }
        )
    }

    fn join_token(&self) -> Option<&str> {
        match self {
            Self::Join { join_token, .. } => Some(join_token.as_str()),
            _ => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Bootstrap { needs_init: true } => "bootstrap-init",
            Self::Bootstrap { needs_init: false } => "bootstrap-reuse",
            Self::Join {
                needs_join: true, ..
            } => "join-init",
            Self::Join {
                needs_join: false, ..
            } => "join-reuse",
            Self::ReuseJoined => "reuse-joined",
        }
    }
}

#[derive(Debug, Clone)]
struct ContainerCloudflare {
    account_id: String,
    zone_id: String,
    zone_name: String,
    hostname: String,
    tunnel_name: String,
    origin_url: String,
    token: String,
    token_source: CloudflareTokenSource,
}

#[derive(Debug, Clone)]
struct ContainerDdns {
    zone_id: String,
    token: String,
    token_file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DefaultVlessEndpointSpec {
    port: u16,
    reality_dest: String,
    server_names: Vec<String>,
    fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DefaultSsEndpointSpec {
    port: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ManagedDefaultEndpointsSpec {
    vless: Option<DefaultVlessEndpointSpec>,
    ss: Option<DefaultSsEndpointSpec>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct ManagedDefaultEndpointsState {
    schema_version: u32,
    #[serde(default)]
    vless_endpoint_id: Option<String>,
    #[serde(default)]
    ss_endpoint_id: Option<String>,
}

impl Default for ManagedDefaultEndpointsState {
    fn default() -> Self {
        Self {
            schema_version: MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION,
            vless_endpoint_id: None,
            ss_endpoint_id: None,
        }
    }
}

#[derive(Debug, Clone)]
struct ContainerSpec {
    node_name: String,
    access_host: String,
    api_base_url: String,
    data_dir: PathBuf,
    bind: SocketAddr,
    xray_api_addr: SocketAddr,
    startup: ContainerStartup,
    bootstrap_admin_token_hash: Option<String>,
    cloudflare: Option<ContainerCloudflare>,
    ddns: Option<ContainerDdns>,
    node_meta_needs_realign: bool,
    default_endpoints: ManagedDefaultEndpointsSpec,
    join_leader_api_base_url: Option<String>,
    runtime_env: BTreeMap<String, String>,
}

pub async fn cmd_container_run(paths: Paths, args: ContainerRunArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };
    let env_map = env::vars().collect::<BTreeMap<String, String>>();
    let binaries = resolve_binary_paths(&paths, &env_map);
    let existing_meta = load_existing_metadata(&paths, &env_map)?;
    let spec = ContainerSpec::from_env_map(&paths, &env_map, existing_meta.as_ref()).await?;

    ensure_container_layout(&paths, &spec, mode)?;
    if let Some(cf) = spec.cloudflare.as_ref() {
        provision_cloudflare(&paths, cf, mode).await?;
    }
    prepare_runtime_inputs(&paths, &spec, existing_meta.as_ref(), mode)?;

    if mode == Mode::DryRun {
        render_dry_run(&spec, &binaries);
        return Ok(());
    }

    ensure_binary_exists(&binaries.xp, "xp")?;
    ensure_binary_exists(&binaries.xray, "xray")?;
    if spec.cloudflare.is_some() {
        ensure_binary_exists(&binaries.cloudflared, "cloudflared")?;
    }

    let mut cloudflared_child = None;
    if spec.startup.needs_join_wait() && spec.cloudflare.is_some() {
        let mut child = spawn_cloudflared(&binaries.cloudflared)?;
        if let Err(err) =
            wait_for_child_startup("cloudflared", &mut child, CLOUDFLARED_STARTUP_GRACE).await
        {
            cleanup_child(&mut child).await;
            return Err(err);
        }
        cloudflared_child = Some(child);
    }

    if let Err(err) = ensure_cluster_bootstrap_state(&binaries.xp, &spec).await {
        cleanup_optional_child(&mut cloudflared_child).await;
        return Err(err);
    }

    let effective_admin_token_hash = match effective_admin_token_hash(&paths, &spec) {
        Ok(hash) => hash,
        Err(err) => {
            cleanup_optional_child(&mut cloudflared_child).await;
            return Err(err);
        }
    };

    let mut xray_child = match spawn_xray(&binaries.xray, &paths) {
        Ok(child) => child,
        Err(err) => {
            cleanup_optional_child(&mut cloudflared_child).await;
            return Err(err);
        }
    };

    if let Err(err) = wait_for_tcp_ready(spec.xray_api_addr, XRAY_READY_TIMEOUT).await {
        cleanup_child(&mut xray_child).await;
        cleanup_optional_child(&mut cloudflared_child).await;
        return Err(err);
    }

    if cloudflared_child.is_none() && spec.cloudflare.is_some() {
        match spawn_cloudflared(&binaries.cloudflared) {
            Ok(child) => cloudflared_child = Some(child),
            Err(err) => {
                cleanup_child(&mut xray_child).await;
                cleanup_optional_child(&mut cloudflared_child).await;
                return Err(err);
            }
        }
    }

    let mut xp_child = match spawn_xp(&binaries.xp, &spec, &effective_admin_token_hash) {
        Ok(child) => child,
        Err(err) => {
            cleanup_child(&mut xray_child).await;
            cleanup_optional_child(&mut cloudflared_child).await;
            return Err(err);
        }
    };

    if let Err(err) = wait_for_local_api_ready(spec.bind.port()).await {
        cleanup_child(&mut xp_child).await;
        cleanup_child(&mut xray_child).await;
        cleanup_optional_child(&mut cloudflared_child).await;
        return Err(err);
    }

    if spec.cloudflare.is_some()
        && let Err(err) = wait_for_public_api_base_url(&spec.api_base_url).await
    {
        cleanup_child(&mut xp_child).await;
        cleanup_child(&mut xray_child).await;
        cleanup_optional_child(&mut cloudflared_child).await;
        return Err(err);
    }

    if let Err(err) = reconcile_runtime_state(&paths, &spec).await {
        cleanup_child(&mut xp_child).await;
        cleanup_child(&mut xray_child).await;
        cleanup_optional_child(&mut cloudflared_child).await;
        return Err(err);
    }

    supervise_children(&mut xp_child, &mut xray_child, cloudflared_child.as_mut()).await
}

impl ContainerSpec {
    async fn from_env_map(
        paths: &Paths,
        env_map: &BTreeMap<String, String>,
        existing_meta: Option<&ClusterMetadata>,
    ) -> Result<Self, ExitError> {
        let node_name = required_env(env_map, "XP_NODE_NAME")?;
        let data_dir = absolute_path_env(env_map, "XP_DATA_DIR", DEFAULT_DATA_DIR)?;
        let bind = socket_addr_env(env_map, "XP_BIND", DEFAULT_BIND)?;
        let xray_api_addr = socket_addr_env(env_map, "XP_XRAY_API_ADDR", DEFAULT_XRAY_API_ADDR)?;
        let join_token = optional_env(env_map, "XP_JOIN_TOKEN");
        let join_leader_api_base_url = join_token
            .as_deref()
            .and_then(decode_join_token_leader_api_base_url);
        let cloudflare_enabled = bool_env(env_map, "XP_ENABLE_CLOUDFLARE", false)?;

        let cloudflare = if cloudflare_enabled {
            Some(build_cloudflare_spec(paths, env_map, &node_name).await?)
        } else {
            None
        };

        let api_base_url = resolve_api_base_url(env_map, cloudflare.as_ref())?;
        let access_host = resolve_access_host(env_map, &api_base_url, cloudflare.as_ref())?;
        let ddns = build_ddns_spec(paths, env_map, &access_host, cloudflare.as_ref()).await?;
        let default_endpoints = ManagedDefaultEndpointsSpec::from_env_map(env_map)?;
        let runtime_env = build_runtime_env(ddns.as_ref());

        let startup = resolve_startup(existing_meta, join_token.as_deref())?;
        let node_meta_needs_realign = existing_meta
            .is_some_and(|meta| node_meta_mismatch(meta, &node_name, &access_host, &api_base_url));

        let bootstrap_admin_token_hash = if startup.requires_bootstrap_token() {
            Some(resolve_bootstrap_admin_token_hash(env_map)?)
        } else {
            None
        };

        Ok(Self {
            node_name,
            access_host,
            api_base_url,
            data_dir,
            bind,
            xray_api_addr,
            startup,
            bootstrap_admin_token_hash,
            cloudflare,
            ddns,
            node_meta_needs_realign,
            default_endpoints,
            join_leader_api_base_url,
            runtime_env,
        })
    }
}

async fn build_cloudflare_spec(
    paths: &Paths,
    env_map: &BTreeMap<String, String>,
    node_name: &str,
) -> Result<ContainerCloudflare, ExitError> {
    let account_id = required_env(env_map, "XP_CLOUDFLARE_ACCOUNT_ID")?;
    let hostname = required_env(env_map, "XP_CLOUDFLARE_HOSTNAME")?.to_ascii_lowercase();
    validate_hostname(&hostname)?;
    let tunnel_name = optional_env(env_map, "XP_CLOUDFLARE_TUNNEL_NAME")
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| format!("xp-{}", sanitize_label(node_name)));

    let (token, token_source) = cloudflare::load_cloudflare_token_for_deploy(paths, None, None)?;
    let api_base = cloudflare::cloudflare_api_base();
    let (zone_id, zone_name) = if let Some(zone_id) = optional_env(env_map, "XP_CLOUDFLARE_ZONE_ID")
    {
        let zone_info = cloudflare::fetch_zone_info(&api_base, &token, &zone_id).await?;
        (zone_id, zone_info.name)
    } else {
        let zone = resolve_zone_from_hostname(&api_base, &token, &account_id, &hostname).await?;
        (zone.id, zone.name)
    };

    if !hostname_in_zone(&hostname, &zone_name) {
        return Err(ExitError::new(
            2,
            format!(
                "invalid_args: XP_CLOUDFLARE_HOSTNAME ({hostname}) does not belong to resolved zone {zone_name}"
            ),
        ));
    }

    Ok(ContainerCloudflare {
        account_id,
        zone_id,
        zone_name,
        hostname,
        tunnel_name,
        origin_url: DEFAULT_CLOUDFLARE_ORIGIN_URL.to_string(),
        token,
        token_source,
    })
}

async fn build_ddns_spec(
    paths: &Paths,
    env_map: &BTreeMap<String, String>,
    access_host: &str,
    cloudflare: Option<&ContainerCloudflare>,
) -> Result<Option<ContainerDdns>, ExitError> {
    if !bool_env(env_map, "XP_CLOUDFLARE_DDNS_ENABLED", false)? {
        return Ok(None);
    }

    validate_hostname(&access_host.to_ascii_lowercase())?;
    let token_file = optional_env(env_map, "XP_CLOUDFLARE_DDNS_TOKEN_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/xp/cloudflare_ddns_api_token"));
    if !token_file.is_absolute() {
        return Err(ExitError::new(
            2,
            "invalid_args: XP_CLOUDFLARE_DDNS_TOKEN_FILE must be an absolute path",
        ));
    }

    let token = cloudflare.map(|cf| cf.token.clone()).unwrap_or_else(|| {
        cloudflare::load_cloudflare_token_for_deploy(paths, None, None)
            .map(|(token, _)| token)
            .unwrap_or_default()
    });
    if token.trim().is_empty() {
        return Err(ExitError::new(
            2,
            "cloudflare_token_missing: DDNS requires CLOUDFLARE_API_TOKEN or persisted tunnel token state",
        ));
    }

    let zone_id = if let Some(zone_id) = optional_env(env_map, "XP_CLOUDFLARE_DDNS_ZONE_ID") {
        zone_id
    } else if let Some(cf) = cloudflare {
        if hostname_in_zone(access_host, &cf.zone_name) {
            cf.zone_id.clone()
        } else {
            return Err(ExitError::new(
                2,
                format!(
                    "invalid_args: XP_ACCESS_HOST ({access_host}) does not belong to Cloudflare tunnel zone {}; set XP_CLOUDFLARE_DDNS_ZONE_ID explicitly",
                    cf.zone_name
                ),
            ));
        }
    } else {
        let api_base = cloudflare::cloudflare_api_base();
        let zone = resolve_zone_from_hostname(&api_base, &token, "", access_host).await?;
        zone.id
    };

    Ok(Some(ContainerDdns {
        zone_id,
        token,
        token_file,
    }))
}

impl ManagedDefaultEndpointsSpec {
    fn from_env_map(env_map: &BTreeMap<String, String>) -> Result<Self, ExitError> {
        Ok(Self {
            vless: parse_default_vless_endpoint_spec(env_map)?,
            ss: parse_default_ss_endpoint_spec(env_map)?,
        })
    }
}

fn parse_default_vless_endpoint_spec(
    env_map: &BTreeMap<String, String>,
) -> Result<Option<DefaultVlessEndpointSpec>, ExitError> {
    let port = optional_port_env(env_map, "XP_DEFAULT_VLESS_PORT")?;
    let reality_dest = optional_env(env_map, "XP_DEFAULT_VLESS_REALITY_DEST");
    let server_names_raw = optional_env(env_map, "XP_DEFAULT_VLESS_SERVER_NAMES");
    let fingerprint = optional_env(env_map, "XP_DEFAULT_VLESS_FINGERPRINT");

    if port.is_none()
        && reality_dest.is_none()
        && server_names_raw.is_none()
        && fingerprint.is_none()
    {
        return Ok(None);
    }

    let port = port.ok_or_else(|| {
        ExitError::new(
            2,
            "invalid_args: XP_DEFAULT_VLESS_PORT is required when managing the default VLESS endpoint",
        )
    })?;
    let reality_dest = reality_dest.ok_or_else(|| {
        ExitError::new(
            2,
            "invalid_args: XP_DEFAULT_VLESS_REALITY_DEST is required when managing the default VLESS endpoint",
        )
    })?;
    let server_names_raw = server_names_raw.ok_or_else(|| {
        ExitError::new(
            2,
            "invalid_args: XP_DEFAULT_VLESS_SERVER_NAMES is required when managing the default VLESS endpoint",
        )
    })?;
    let server_names = parse_server_names(&server_names_raw)?;
    let fingerprint = fingerprint.unwrap_or_else(|| "chrome".to_string());

    Ok(Some(DefaultVlessEndpointSpec {
        port,
        reality_dest,
        server_names,
        fingerprint,
    }))
}

fn parse_default_ss_endpoint_spec(
    env_map: &BTreeMap<String, String>,
) -> Result<Option<DefaultSsEndpointSpec>, ExitError> {
    Ok(
        optional_port_env(env_map, "XP_DEFAULT_SS_PORT")?
            .map(|port| DefaultSsEndpointSpec { port }),
    )
}

fn parse_server_names(value: &str) -> Result<Vec<String>, ExitError> {
    let mut out = Vec::new();
    for raw in value.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        validate_reality_server_name(item).map_err(|reason| {
            ExitError::new(
                2,
                format!("invalid_args: XP_DEFAULT_VLESS_SERVER_NAMES contains invalid server name {item}: {reason}"),
            )
        })?;
        out.push(item.to_string());
    }
    if out.is_empty() {
        return Err(ExitError::new(
            2,
            "invalid_args: XP_DEFAULT_VLESS_SERVER_NAMES must contain at least one hostname",
        ));
    }
    Ok(out)
}

fn build_runtime_env(ddns: Option<&ContainerDdns>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(ddns) = ddns {
        out.insert("XP_CLOUDFLARE_DDNS_ENABLED".to_string(), "true".to_string());
        out.insert(
            "XP_CLOUDFLARE_DDNS_ZONE_ID".to_string(),
            ddns.zone_id.clone(),
        );
        out.insert(
            "XP_CLOUDFLARE_DDNS_TOKEN_FILE".to_string(),
            ddns.token_file.display().to_string(),
        );
    }
    out
}

fn resolve_startup(
    existing_meta: Option<&ClusterMetadata>,
    join_token: Option<&str>,
) -> Result<ContainerStartup, ExitError> {
    if let Some(meta) = existing_meta {
        if meta.should_bootstrap_raft() {
            if join_token.is_some() {
                return Err(ExitError::new(
                    2,
                    "invalid_args: XP_JOIN_TOKEN is not allowed for an existing bootstrap node",
                ));
            }
            return Ok(ContainerStartup::Bootstrap { needs_init: false });
        }
        if let Some(token) = join_token {
            let expected_node_id = ClusterMetadata::expected_join_node_id(token).map_err(|e| {
                ExitError::new(2, format!("invalid_args: decode XP_JOIN_TOKEN: {e}"))
            })?;
            if meta.node_id != expected_node_id {
                return Err(ExitError::new(
                    2,
                    format!(
                        "invalid_args: XP_JOIN_TOKEN targets node_id {expected_node_id}, but existing data belongs to {}",
                        meta.node_id
                    ),
                ));
            }
            return Ok(ContainerStartup::Join {
                join_token: token.to_string(),
                needs_join: false,
            });
        }
        return Ok(ContainerStartup::ReuseJoined);
    }

    if let Some(token) = join_token {
        return Ok(ContainerStartup::Join {
            join_token: token.to_string(),
            needs_join: true,
        });
    }

    Ok(ContainerStartup::Bootstrap { needs_init: true })
}

fn resolve_bootstrap_admin_token_hash(
    env_map: &BTreeMap<String, String>,
) -> Result<String, ExitError> {
    if let Some(hash) = optional_env(env_map, "XP_ADMIN_TOKEN_HASH") {
        if parse_admin_token_hash(&hash).is_none() {
            return Err(ExitError::new(
                2,
                "invalid_args: XP_ADMIN_TOKEN_HASH is present but invalid",
            ));
        }
        return Ok(hash);
    }

    if let Some(token) = optional_env(env_map, "XP_ADMIN_TOKEN") {
        if token.trim().is_empty() {
            return Err(ExitError::new(2, "invalid_args: XP_ADMIN_TOKEN is empty"));
        }
        return hash_admin_token_argon2id(&token)
            .map(|hash| hash.as_str().to_string())
            .map_err(|e| ExitError::new(2, format!("invalid_args: hash XP_ADMIN_TOKEN: {e}")));
    }

    Err(ExitError::new(
        2,
        "invalid_args: bootstrap mode requires XP_ADMIN_TOKEN or XP_ADMIN_TOKEN_HASH",
    ))
}

fn resolve_api_base_url(
    env_map: &BTreeMap<String, String>,
    cloudflare: Option<&ContainerCloudflare>,
) -> Result<String, ExitError> {
    if let Some(value) = optional_env(env_map, "XP_API_BASE_URL") {
        validate_https_origin(&value)?;
        if let Some(cf) = cloudflare {
            let expected = format!("https://{}", cf.hostname);
            if value != expected {
                return Err(ExitError::new(
                    2,
                    format!(
                        "invalid_args: XP_API_BASE_URL must match Cloudflare hostname ({expected}) when XP_ENABLE_CLOUDFLARE=true"
                    ),
                ));
            }
        }
        return Ok(value);
    }

    if let Some(cf) = cloudflare {
        return Ok(format!("https://{}", cf.hostname));
    }

    Err(ExitError::new(
        2,
        "invalid_args: XP_API_BASE_URL is required when XP_ENABLE_CLOUDFLARE is false",
    ))
}

fn resolve_access_host(
    env_map: &BTreeMap<String, String>,
    api_base_url: &str,
    cloudflare: Option<&ContainerCloudflare>,
) -> Result<String, ExitError> {
    if let Some(value) = optional_env(env_map, "XP_ACCESS_HOST") {
        return Ok(value);
    }
    if let Some(cf) = cloudflare {
        return Ok(cf.hostname.clone());
    }
    let url = Url::parse(api_base_url).map_err(|e| {
        ExitError::new(
            2,
            format!("invalid_args: XP_API_BASE_URL parse failed: {e}"),
        )
    })?;
    let host = url
        .host_str()
        .ok_or_else(|| ExitError::new(2, "invalid_args: XP_API_BASE_URL missing host"))?;
    Ok(host.to_string())
}

fn node_meta_mismatch(
    meta: &ClusterMetadata,
    node_name: &str,
    access_host: &str,
    api_base_url: &str,
) -> bool {
    meta.node_name != node_name
        || meta.access_host != access_host
        || meta.api_base_url != api_base_url
}

fn render_node_meta_diffs(
    meta: &ClusterMetadata,
    node_name: &str,
    access_host: &str,
    api_base_url: &str,
) -> Vec<String> {
    let mut diffs = Vec::new();
    if meta.node_name != node_name {
        diffs.push(format!("node_name: {} -> {node_name}", meta.node_name));
    }
    if meta.access_host != access_host {
        diffs.push(format!(
            "access_host: {} -> {access_host}",
            meta.access_host
        ));
    }
    if meta.api_base_url != api_base_url {
        diffs.push(format!(
            "api_base_url: {} -> {api_base_url}",
            meta.api_base_url
        ));
    }
    diffs
}

fn effective_admin_token_hash(paths: &Paths, spec: &ContainerSpec) -> Result<String, ExitError> {
    match &spec.startup {
        ContainerStartup::Bootstrap { .. } => spec
            .bootstrap_admin_token_hash
            .clone()
            .ok_or_else(|| ExitError::new(2, "invalid_args: bootstrap admin token hash missing")),
        ContainerStartup::Join { .. } | ContainerStartup::ReuseJoined => {
            read_cluster_admin_token_hash(paths, &spec.data_dir)
        }
    }
}

fn prepare_runtime_inputs(
    paths: &Paths,
    spec: &ContainerSpec,
    existing_meta: Option<&ClusterMetadata>,
    mode: Mode,
) -> Result<(), ExitError> {
    if let Some(ddns) = spec.ddns.as_ref() {
        ensure_ddns_runtime_token_file(paths, ddns, mode)?;
    }

    if spec.node_meta_needs_realign
        && let Some(meta) = existing_meta
    {
        let diffs =
            render_node_meta_diffs(meta, &spec.node_name, &spec.access_host, &spec.api_base_url);
        if mode == Mode::DryRun {
            eprintln!("would realign persisted node metadata:");
            for diff in diffs {
                eprintln!("  - {diff}");
            }
            return Ok(());
        }
        let abs_data_dir = paths.map_abs(&spec.data_dir);
        let mut next = meta.clone();
        next.node_name = spec.node_name.clone();
        next.access_host = spec.access_host.clone();
        next.api_base_url = spec.api_base_url.clone();
        next.save(&abs_data_dir)
            .map_err(|e| ExitError::new(4, format!("cluster_metadata_error: {e}")))?;
    }

    Ok(())
}

fn ensure_ddns_runtime_token_file(
    paths: &Paths,
    ddns: &ContainerDdns,
    mode: Mode,
) -> Result<(), ExitError> {
    let target = paths.map_abs(&ddns.token_file);
    if mode == Mode::DryRun {
        eprintln!("would write: {}", target.display());
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        ensure_dir(parent).map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    }
    write_string_if_changed(&target, &(ddns.token.trim().to_string() + "\n"))
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    Ok(())
}

fn read_cluster_admin_token_hash(paths: &Paths, data_dir: &Path) -> Result<String, ExitError> {
    let abs_data_dir = paths.map_abs(data_dir);
    let cluster_paths = ClusterPaths::new(&abs_data_dir);
    let raw = fs::read_to_string(&cluster_paths.admin_token_hash).map_err(|_| {
        ExitError::new(
            2,
            "admin_token_missing: cluster admin token hash not found under XP_DATA_DIR",
        )
    })?;
    let hash = raw.trim();
    if parse_admin_token_hash(hash).is_none() {
        return Err(ExitError::new(
            2,
            "admin_token_invalid: cluster admin token hash is invalid",
        ));
    }
    Ok(hash.to_string())
}

fn ensure_container_layout(
    paths: &Paths,
    spec: &ContainerSpec,
    mode: Mode,
) -> Result<(), ExitError> {
    let data_dir = paths.map_abs(&spec.data_dir);
    let container_state_dir = managed_default_endpoints_state_path(&data_dir)
        .parent()
        .expect("managed endpoints state file has a parent")
        .to_path_buf();
    let xp_dir = paths.etc_xp_dir();
    let xray_dir = paths.etc_xray_dir();
    let cloudflared_dir = paths.etc_cloudflared_dir();
    let xp_ops_cloudflare_dir = paths.etc_xp_ops_cloudflare_dir();
    let dirs = [
        data_dir.as_path(),
        container_state_dir.as_path(),
        xp_dir.as_path(),
        xray_dir.as_path(),
        cloudflared_dir.as_path(),
        xp_ops_cloudflare_dir.as_path(),
    ];

    for dir in dirs {
        if mode == Mode::DryRun {
            continue;
        }
        ensure_dir(dir).map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    }

    if mode == Mode::Real {
        init::write_static_xray_config(paths)?;
    }
    Ok(())
}

async fn provision_cloudflare(
    paths: &Paths,
    cf: &ContainerCloudflare,
    mode: Mode,
) -> Result<(), ExitError> {
    cloudflare::cmd_cloudflare_provision_container(
        paths.clone(),
        CloudflareProvisionArgs {
            tunnel_name: Some(cf.tunnel_name.clone()),
            account_id: cf.account_id.clone(),
            zone_id: cf.zone_id.clone(),
            hostname: cf.hostname.clone(),
            origin_url: cf.origin_url.clone(),
            dns_record_id_override: None,
            tunnel_id_override: None,
            enable: false,
            no_enable: true,
            dry_run: mode == Mode::DryRun,
        },
        cf.token.clone(),
    )
    .await
}

async fn ensure_cluster_bootstrap_state(
    xp_bin: &Path,
    spec: &ContainerSpec,
) -> Result<(), ExitError> {
    if spec.startup.needs_init() {
        run_xp_command(xp_bin, &["init"], spec, None).await?;
    }

    if spec.startup.needs_join() {
        let join_token = spec
            .startup
            .join_token()
            .ok_or_else(|| ExitError::new(2, "invalid_args: XP_JOIN_TOKEN missing"))?;
        run_xp_command(xp_bin, &["join", "--token", join_token], spec, None).await?;
    }
    Ok(())
}

async fn reconcile_runtime_state(paths: &Paths, spec: &ContainerSpec) -> Result<(), ExitError> {
    let local_base_url = local_xp_base_url(spec.bind.port());
    let control_plane_base_url = spec
        .join_leader_api_base_url
        .as_deref()
        .unwrap_or(local_base_url.as_str());

    if spec.node_meta_needs_realign {
        crate::ops::xp::sync_node_meta_runtime(
            paths,
            &spec.data_dir,
            &spec.node_name,
            &spec.access_host,
            &spec.api_base_url,
            control_plane_base_url,
            Mode::Real,
        )
        .await?;
    }

    reconcile_managed_default_endpoints(paths, spec, control_plane_base_url).await
}

async fn reconcile_managed_default_endpoints(
    paths: &Paths,
    spec: &ContainerSpec,
    xp_base_url: &str,
) -> Result<(), ExitError> {
    let abs_data_dir = paths.map_abs(&spec.data_dir);
    let cluster_meta = ClusterMetadata::load(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_metadata_error: {e}")))?;
    let cluster_ca_key_pem = cluster_meta
        .read_cluster_ca_key_pem(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_ca_key_error: {e}")))?
        .ok_or_else(|| ExitError::new(5, "cluster_ca_key_missing"))?;
    let cluster_ca_pem = cluster_meta
        .read_cluster_ca_pem(&abs_data_dir)
        .map_err(|e| ExitError::new(5, format!("cluster_ca_error: {e}")))?;

    let client = crate::ops::xp::build_xp_ops_http_client(xp_base_url, &cluster_ca_pem)?;
    let endpoints =
        crate::ops::xp::fetch_admin_endpoints_internal(&client, xp_base_url, &cluster_ca_key_pem)
            .await?;
    let node_endpoints: Vec<Endpoint> = endpoints
        .into_iter()
        .filter(|endpoint| endpoint.node_id == cluster_meta.node_id)
        .collect();

    let mut state = load_managed_default_endpoints_state(&abs_data_dir)?;

    state.vless_endpoint_id = reconcile_one_managed_endpoint(
        &client,
        xp_base_url,
        &cluster_ca_key_pem,
        &cluster_meta.node_id,
        ManagedDefaultEndpointKind::Vless,
        spec.default_endpoints.vless.as_ref(),
        state.vless_endpoint_id.as_deref(),
        &node_endpoints,
    )
    .await?;

    state.ss_endpoint_id = reconcile_one_managed_endpoint(
        &client,
        xp_base_url,
        &cluster_ca_key_pem,
        &cluster_meta.node_id,
        ManagedDefaultEndpointKind::Ss,
        spec.default_endpoints.ss.as_ref(),
        state.ss_endpoint_id.as_deref(),
        &node_endpoints,
    )
    .await?;

    persist_managed_default_endpoints_state(&abs_data_dir, &state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedDefaultEndpointKind {
    Vless,
    Ss,
}

#[allow(clippy::too_many_arguments)]
async fn reconcile_one_managed_endpoint<T>(
    client: &reqwest::Client,
    xp_base_url: &str,
    cluster_ca_key_pem: &str,
    node_id: &str,
    kind: ManagedDefaultEndpointKind,
    desired: Option<&T>,
    managed_endpoint_id: Option<&str>,
    node_endpoints: &[Endpoint],
) -> Result<Option<String>, ExitError>
where
    T: ManagedEndpointSpec,
{
    let same_kind: Vec<&Endpoint> = node_endpoints
        .iter()
        .filter(|endpoint| endpoint_matches_kind(endpoint, kind))
        .collect();
    let managed_current = managed_endpoint_id.and_then(|endpoint_id| {
        same_kind
            .iter()
            .find(|endpoint| endpoint.endpoint_id == endpoint_id)
            .copied()
    });

    let Some(desired) = desired else {
        if let Some(endpoint) = managed_current {
            eprintln!(
                "container reconcile: deleting managed {} endpoint {}",
                kind.label(),
                endpoint.endpoint_id
            );
            crate::ops::xp::internal_client_write(
                client,
                xp_base_url,
                cluster_ca_key_pem,
                crate::state::DesiredStateCommand::DeleteEndpoint {
                    endpoint_id: endpoint.endpoint_id.clone(),
                },
            )
            .await?;
        }
        return Ok(None);
    };

    if let Some(endpoint) = managed_current {
        let next = desired.reconcile_existing(endpoint)?;
        if &next != endpoint {
            eprintln!(
                "container reconcile: updating managed {} endpoint {}",
                kind.label(),
                endpoint.endpoint_id
            );
            crate::ops::xp::internal_client_write(
                client,
                xp_base_url,
                cluster_ca_key_pem,
                crate::state::DesiredStateCommand::UpsertEndpoint { endpoint: next },
            )
            .await?;
        }
        return Ok(Some(endpoint.endpoint_id.clone()));
    }

    match same_kind.as_slice() {
        [] => {
            let endpoint = desired.create_new(node_id.to_string())?;
            let endpoint_id = endpoint.endpoint_id.clone();
            eprintln!(
                "container reconcile: creating managed {} endpoint {}",
                kind.label(),
                endpoint_id
            );
            crate::ops::xp::internal_client_write(
                client,
                xp_base_url,
                cluster_ca_key_pem,
                crate::state::DesiredStateCommand::UpsertEndpoint { endpoint },
            )
            .await?;
            Ok(Some(endpoint_id))
        }
        [endpoint] => {
            let next = desired.reconcile_existing(endpoint)?;
            let endpoint_id = endpoint.endpoint_id.clone();
            if &next != *endpoint {
                eprintln!(
                    "container reconcile: adopting and updating {} endpoint {}",
                    kind.label(),
                    endpoint_id
                );
                crate::ops::xp::internal_client_write(
                    client,
                    xp_base_url,
                    cluster_ca_key_pem,
                    crate::state::DesiredStateCommand::UpsertEndpoint { endpoint: next },
                )
                .await?;
            } else {
                eprintln!(
                    "container reconcile: adopting existing {} endpoint {}",
                    kind.label(),
                    endpoint_id
                );
            }
            Ok(Some(endpoint_id))
        }
        _ => Err(ExitError::new(
            5,
            format!(
                "container_reconcile_failed: multiple {} endpoints already exist on this node; set only one managed endpoint or clean them up manually",
                kind.label()
            ),
        )),
    }
}

trait ManagedEndpointSpec {
    fn create_new(&self, node_id: String) -> Result<Endpoint, ExitError>;
    fn reconcile_existing(&self, endpoint: &Endpoint) -> Result<Endpoint, ExitError>;
}

impl ManagedEndpointSpec for DefaultVlessEndpointSpec {
    fn create_new(&self, node_id: String) -> Result<Endpoint, ExitError> {
        let endpoint_id = new_ulid_string();
        let tag = managed_endpoint_tag(ManagedDefaultEndpointKind::Vless, &endpoint_id);
        let mut rng = OsRng;
        let keypair = generate_reality_keypair(&mut rng);
        let short_id = generate_short_id_16hex(&mut rng);
        let meta = VlessRealityVisionTcpEndpointMeta {
            reality: self.reality_config(),
            reality_keys: RealityKeys {
                private_key: keypair.private_key,
                public_key: keypair.public_key,
            },
            short_ids: vec![short_id.clone()],
            active_short_id: short_id,
        };
        Ok(Endpoint {
            endpoint_id,
            node_id,
            tag,
            kind: EndpointKind::VlessRealityVisionTcp,
            port: self.port,
            meta: serde_json::to_value(meta)
                .map_err(|e| ExitError::new(5, format!("json_error: {e}")))?,
        })
    }

    fn reconcile_existing(&self, endpoint: &Endpoint) -> Result<Endpoint, ExitError> {
        if endpoint.kind != EndpointKind::VlessRealityVisionTcp {
            return Err(ExitError::new(
                5,
                format!(
                    "container_reconcile_failed: endpoint {} is not a VLESS endpoint",
                    endpoint.endpoint_id
                ),
            ));
        }
        let mut meta: VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta.clone()).map_err(|e| {
                ExitError::new(
                    5,
                    format!(
                        "container_reconcile_failed: parse VLESS endpoint {} metadata: {e}",
                        endpoint.endpoint_id
                    ),
                )
            })?;
        meta.reality = self.reality_config();

        let mut next = endpoint.clone();
        next.port = self.port;
        next.meta = serde_json::to_value(meta)
            .map_err(|e| ExitError::new(5, format!("json_error: {e}")))?;
        Ok(next)
    }
}

impl DefaultVlessEndpointSpec {
    fn reality_config(&self) -> RealityConfig {
        RealityConfig {
            dest: self.reality_dest.clone(),
            server_names: self.server_names.clone(),
            server_names_source: RealityServerNamesSource::Manual,
            fingerprint: self.fingerprint.clone(),
        }
    }
}

impl ManagedEndpointSpec for DefaultSsEndpointSpec {
    fn create_new(&self, node_id: String) -> Result<Endpoint, ExitError> {
        let endpoint_id = new_ulid_string();
        let tag = managed_endpoint_tag(ManagedDefaultEndpointKind::Ss, &endpoint_id);
        let mut rng = OsRng;
        let meta = Ss2022EndpointMeta {
            method: SS2022_METHOD_2022_BLAKE3_AES_128_GCM.to_string(),
            server_psk_b64: generate_ss2022_psk_b64(&mut rng),
        };
        Ok(Endpoint {
            endpoint_id,
            node_id,
            tag,
            kind: EndpointKind::Ss2022_2022Blake3Aes128Gcm,
            port: self.port,
            meta: serde_json::to_value(meta)
                .map_err(|e| ExitError::new(5, format!("json_error: {e}")))?,
        })
    }

    fn reconcile_existing(&self, endpoint: &Endpoint) -> Result<Endpoint, ExitError> {
        if endpoint.kind != EndpointKind::Ss2022_2022Blake3Aes128Gcm {
            return Err(ExitError::new(
                5,
                format!(
                    "container_reconcile_failed: endpoint {} is not an SS2022 endpoint",
                    endpoint.endpoint_id
                ),
            ));
        }
        let _: Ss2022EndpointMeta = serde_json::from_value(endpoint.meta.clone()).map_err(|e| {
            ExitError::new(
                5,
                format!(
                    "container_reconcile_failed: parse SS2022 endpoint {} metadata: {e}",
                    endpoint.endpoint_id
                ),
            )
        })?;
        let mut next = endpoint.clone();
        next.port = self.port;
        Ok(next)
    }
}

fn endpoint_matches_kind(endpoint: &Endpoint, kind: ManagedDefaultEndpointKind) -> bool {
    match kind {
        ManagedDefaultEndpointKind::Vless => endpoint.kind == EndpointKind::VlessRealityVisionTcp,
        ManagedDefaultEndpointKind::Ss => endpoint.kind == EndpointKind::Ss2022_2022Blake3Aes128Gcm,
    }
}

fn managed_endpoint_tag(kind: ManagedDefaultEndpointKind, endpoint_id: &str) -> String {
    let prefix = match kind {
        ManagedDefaultEndpointKind::Vless => "vless-vision",
        ManagedDefaultEndpointKind::Ss => "ss2022",
    };
    format!("{prefix}-{endpoint_id}")
}

impl ManagedDefaultEndpointKind {
    fn label(&self) -> &'static str {
        match self {
            Self::Vless => "vless_reality_vision_tcp",
            Self::Ss => "ss2022_2022_blake3_aes_128_gcm",
        }
    }
}

fn managed_default_endpoints_state_path(data_dir: &Path) -> PathBuf {
    data_dir
        .join("container")
        .join("managed_default_endpoints.json")
}

fn load_managed_default_endpoints_state(
    data_dir: &Path,
) -> Result<ManagedDefaultEndpointsState, ExitError> {
    let path = managed_default_endpoints_state_path(data_dir);
    if !path.exists() {
        return Ok(ManagedDefaultEndpointsState::default());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    let state: ManagedDefaultEndpointsState = serde_json::from_str(&raw)
        .map_err(|e| ExitError::new(5, format!("container_state_error: {e}")))?;
    if state.schema_version != MANAGED_DEFAULT_ENDPOINTS_SCHEMA_VERSION {
        return Err(ExitError::new(
            5,
            format!(
                "container_state_error: unsupported managed endpoint state schema_version {}",
                state.schema_version
            ),
        ));
    }
    Ok(state)
}

fn persist_managed_default_endpoints_state(
    data_dir: &Path,
    state: &ManagedDefaultEndpointsState,
) -> Result<(), ExitError> {
    let path = managed_default_endpoints_state_path(data_dir);
    if state.vless_endpoint_id.is_none() && state.ss_endpoint_id.is_none() {
        match fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(ExitError::new(4, format!("filesystem_error: {err}"))),
        }
    }
    if let Some(parent) = path.parent() {
        ensure_dir(parent).map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    }
    let raw = serde_json::to_string_pretty(state)
        .map_err(|e| ExitError::new(5, format!("container_state_error: {e}")))?;
    write_string_if_changed(&path, &(raw + "\n"))
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    Ok(())
}

fn local_xp_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

async fn run_xp_command(
    xp_bin: &Path,
    subcommand: &[&str],
    spec: &ContainerSpec,
    admin_token_hash: Option<&str>,
) -> Result<(), ExitError> {
    let mut cmd = Command::new(xp_bin);
    cmd.envs(&spec.runtime_env);
    cmd.args(subcommand)
        .arg("--data-dir")
        .arg(&spec.data_dir)
        .arg("--node-name")
        .arg(&spec.node_name)
        .arg("--access-host")
        .arg(&spec.access_host)
        .arg("--api-base-url")
        .arg(&spec.api_base_url)
        .arg("--xray-api-addr")
        .arg(spec.xray_api_addr.to_string())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(hash) = admin_token_hash {
        cmd.arg("--admin-token-hash").arg(hash);
    }
    let status = cmd
        .status()
        .await
        .map_err(|e| ExitError::new(6, format!("container_start_failed: run xp command: {e}")))?;
    if status.success() {
        return Ok(());
    }
    Err(exit_status_error(
        "xp one-shot command",
        status,
        status.code().unwrap_or(1),
    ))
}

fn spawn_xray(xray_bin: &Path, paths: &Paths) -> Result<Child, ExitError> {
    Command::new(xray_bin)
        .arg("run")
        .arg("-c")
        .arg(paths.etc_xray_config())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| ExitError::new(6, format!("container_start_failed: spawn xray: {e}")))
}

fn spawn_cloudflared(cloudflared_bin: &Path) -> Result<Child, ExitError> {
    Command::new(cloudflared_bin)
        .arg("--no-autoupdate")
        .arg("--config")
        .arg("/etc/cloudflared/config.yml")
        .arg("tunnel")
        .arg("run")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| ExitError::new(6, format!("container_start_failed: spawn cloudflared: {e}")))
}

fn spawn_xp(
    xp_bin: &Path,
    spec: &ContainerSpec,
    admin_token_hash: &str,
) -> Result<Child, ExitError> {
    let mut cmd = Command::new(xp_bin);
    cmd.envs(&spec.runtime_env);
    cmd.arg("run")
        .arg("--bind")
        .arg(spec.bind.to_string())
        .arg("--data-dir")
        .arg(&spec.data_dir)
        .arg("--node-name")
        .arg(&spec.node_name)
        .arg("--access-host")
        .arg(&spec.access_host)
        .arg("--api-base-url")
        .arg(&spec.api_base_url)
        .arg("--xray-api-addr")
        .arg(spec.xray_api_addr.to_string())
        .arg("--admin-token-hash")
        .arg(admin_token_hash)
        .arg("--xray-restart-mode")
        .arg("none")
        .arg("--cloudflared-restart-mode")
        .arg("none")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    cmd.spawn()
        .map_err(|e| ExitError::new(6, format!("container_start_failed: spawn xp: {e}")))
}

async fn wait_for_tcp_ready(addr: SocketAddr, timeout_window: Duration) -> Result<(), ExitError> {
    let deadline = Instant::now() + timeout_window;
    loop {
        match TcpStream::connect(addr).await {
            Ok(_) => return Ok(()),
            Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(300)).await,
            Err(err) => {
                return Err(ExitError::new(
                    6,
                    format!("container_start_failed: xray gRPC not ready at {addr}: {err}"),
                ));
            }
        }
    }
}

fn public_api_probe_status_is_ready(status: reqwest::StatusCode) -> bool {
    status.as_u16() != 530
}

async fn wait_for_local_api_ready(port: u16) -> Result<(), ExitError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| ExitError::new(5, format!("http_error: build client: {e}")))?;
    let deadline = Instant::now() + LOCAL_API_READY_TIMEOUT;
    let health_url = format!("{}/api/health", local_xp_base_url(port));

    loop {
        match client.get(&health_url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            Ok(_) | Err(_) if Instant::now() < deadline => sleep(LOCAL_API_READY_DELAY).await,
            Ok(resp) => {
                return Err(ExitError::new(
                    6,
                    format!(
                        "container_start_failed: local xp api not ready at {health_url}: http {}",
                        resp.status().as_u16()
                    ),
                ));
            }
            Err(err) => {
                return Err(ExitError::new(
                    6,
                    format!(
                        "container_start_failed: local xp api not ready at {health_url}: {err}"
                    ),
                ));
            }
        }
    }
}

async fn wait_for_child_startup(
    label: &str,
    child: &mut Child,
    grace: Duration,
) -> Result<(), ExitError> {
    match timeout(grace, child.wait()).await {
        Err(_) => Ok(()),
        Ok(Ok(status)) => Err(exit_status_error(
            label,
            status,
            child_exit_code(status, 1).max(1),
        )),
        Ok(Err(err)) => Err(ExitError::new(
            6,
            format!("process_error: wait {label}: {err}"),
        )),
    }
}

async fn wait_for_public_api_base_url(api_base_url: &str) -> Result<(), ExitError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PUBLIC_API_PROBE_TIMEOUT)
        .build()
        .map_err(|e| ExitError::new(5, format!("http_error: build client: {e}")))?;
    let health_url = format!("{}/health", api_base_url.trim_end_matches('/'));
    let mut last_observation = "no attempts executed".to_string();

    for attempt in 0..PUBLIC_API_PROBE_ATTEMPTS {
        match client.get(&health_url).send().await {
            Ok(resp) => {
                if public_api_probe_status_is_ready(resp.status()) {
                    return Ok(());
                }
                last_observation = format!("http {}", resp.status().as_u16());
            }
            Err(err) => {
                last_observation = err.to_string();
            }
        }
        if attempt + 1 < PUBLIC_API_PROBE_ATTEMPTS {
            sleep(PUBLIC_API_PROBE_DELAY).await;
        }
    }

    Err(ExitError::new(
        6,
        format!(
            "container_start_failed: public api-base-url is not ready after local xp startup: {last_observation}"
        ),
    ))
}

async fn supervise_children(
    xp_child: &mut Child,
    xray_child: &mut Child,
    mut cloudflared_child: Option<&mut Child>,
) -> Result<(), ExitError> {
    #[cfg(unix)]
    let mut sigterm = signal(SignalKind::terminate())
        .map_err(|e| ExitError::new(6, format!("signal_error: {e}")))?;

    let sigterm_wait = async {
        #[cfg(unix)]
        {
            let _ = sigterm.recv().await;
        }
        #[cfg(not(unix))]
        pending::<()>().await;
    };
    tokio::select! {
        status = xp_child.wait() => {
            let status = status.map_err(|e| ExitError::new(6, format!("process_error: wait xp: {e}")))?;
            cleanup_child(xray_child).await;
            cleanup_optional_child_ref(&mut cloudflared_child).await;
            if status.success() {
                return Ok(());
            }
            Err(exit_status_error("xp", status, child_exit_code(status, 1)))
        }
        status = xray_child.wait() => {
            let status = status.map_err(|e| ExitError::new(6, format!("process_error: wait xray: {e}")))?;
            cleanup_child(xp_child).await;
            cleanup_optional_child_ref(&mut cloudflared_child).await;
            Err(exit_status_error("xray", status, child_exit_code(status, 1).max(1)))
        }
        status = async {
            if let Some(child) = cloudflared_child.as_deref_mut() {
                Some(child.wait().await)
            } else {
                pending::<Option<io::Result<std::process::ExitStatus>>>().await
            }
        } => {
            let status = status.expect("cloudflared future returns Some when active")
                .map_err(|e| ExitError::new(6, format!("process_error: wait cloudflared: {e}")))?;
            cleanup_child(xp_child).await;
            cleanup_child(xray_child).await;
            Err(exit_status_error("cloudflared", status, child_exit_code(status, 1).max(1)))
        }
        _ = tokio::signal::ctrl_c() => {
            cleanup_child(xp_child).await;
            cleanup_child(xray_child).await;
            cleanup_optional_child_ref(&mut cloudflared_child).await;
            Ok(())
        }
        _ = sigterm_wait => {
            cleanup_child(xp_child).await;
            cleanup_child(xray_child).await;
            cleanup_optional_child_ref(&mut cloudflared_child).await;
            Ok(())
        }
    }
}

async fn cleanup_optional_child(child: &mut Option<Child>) {
    if let Some(child) = child.as_mut() {
        cleanup_child(child).await;
    }
}

async fn cleanup_optional_child_ref(child: &mut Option<&mut Child>) {
    if let Some(child) = child.as_deref_mut() {
        cleanup_child(child).await;
    }
}

async fn cleanup_child(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(_) => {}
    }

    #[cfg(unix)]
    {
        let _ = send_sigterm(child);
    }

    match timeout(CHILD_SHUTDOWN_TIMEOUT, child.wait()).await {
        Ok(_) => {}
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
}

#[cfg(unix)]
fn send_sigterm(child: &Child) -> io::Result<()> {
    let Some(pid) = child.id() else {
        return Ok(());
    };
    let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if rc == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(err)
}

fn child_exit_code(status: std::process::ExitStatus, default_code: i32) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    if let Some(signal) = status.signal() {
        return 128 + signal;
    }
    default_code
}

fn exit_status_error(
    label: &str,
    status: std::process::ExitStatus,
    default_code: i32,
) -> ExitError {
    let code = child_exit_code(status, default_code);
    if let Some(exit_code) = status.code() {
        return ExitError::new(
            if label == "xp" {
                exit_code
            } else {
                exit_code.max(1)
            },
            format!("container_failed: {label} exited with code {exit_code}"),
        );
    }
    #[cfg(unix)]
    if let Some(signal) = status.signal() {
        return ExitError::new(
            if label == "xp" {
                128 + signal
            } else {
                (128 + signal).max(1)
            },
            format!("container_failed: {label} exited via signal {signal}"),
        );
    }
    ExitError::new(
        code.max(1),
        format!("container_failed: {label} exited unexpectedly"),
    )
}

fn resolve_binary_paths(paths: &Paths, env_map: &BTreeMap<String, String>) -> BinaryPaths {
    let xp = env_map
        .get("XP_OPS_CONTAINER_XP_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.usr_local_bin_xp());
    let xray = env_map
        .get("XP_OPS_CONTAINER_XRAY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.usr_local_bin_xray());
    let cloudflared = env_map
        .get("XP_OPS_CONTAINER_CLOUDFLARED_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let usr_bin = paths.map_abs(Path::new("/usr/bin/cloudflared"));
            if usr_bin.exists() {
                usr_bin
            } else {
                paths.map_abs(Path::new("/usr/local/bin/cloudflared"))
            }
        });
    BinaryPaths {
        xp,
        xray,
        cloudflared,
    }
}

fn ensure_binary_exists(path: &Path, label: &str) -> Result<(), ExitError> {
    if path.exists() {
        return Ok(());
    }
    Err(ExitError::new(
        6,
        format!(
            "container_start_failed: missing {label} binary at {}",
            path.display()
        ),
    ))
}

fn decode_join_token_leader_api_base_url(join_token: &str) -> Option<String> {
    crate::cluster_identity::JoinToken::decode_base64url_json(join_token)
        .ok()
        .map(|token| token.leader_api_base_url)
}

fn load_existing_metadata(
    paths: &Paths,
    env_map: &BTreeMap<String, String>,
) -> Result<Option<ClusterMetadata>, ExitError> {
    let data_dir = absolute_path_env(env_map, "XP_DATA_DIR", DEFAULT_DATA_DIR)?;
    let metadata_path = ClusterPaths::new(&paths.map_abs(&data_dir)).metadata_json;
    if !metadata_path.exists() {
        return Ok(None);
    }
    ClusterMetadata::load(&paths.map_abs(&data_dir))
        .map(Some)
        .map_err(|e| ExitError::new(2, format!("cluster_metadata_error: {e}")))
}

fn required_env(env_map: &BTreeMap<String, String>, key: &str) -> Result<String, ExitError> {
    let value = optional_env(env_map, key)
        .ok_or_else(|| ExitError::new(2, format!("invalid_args: {key} is required")))?;
    if value.trim().is_empty() {
        return Err(ExitError::new(2, format!("invalid_args: {key} is empty")));
    }
    Ok(value)
}

fn optional_env(env_map: &BTreeMap<String, String>, key: &str) -> Option<String> {
    env_map
        .get(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn bool_env(
    env_map: &BTreeMap<String, String>,
    key: &str,
    default_value: bool,
) -> Result<bool, ExitError> {
    let Some(value) = env_map.get(key) else {
        return Ok(default_value);
    };
    parse_boolish(key, value)
}

fn parse_boolish(key: &str, value: &str) -> Result<bool, ExitError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(ExitError::new(
            2,
            format!("invalid_args: {key} must be a boolish value, got {other}"),
        )),
    }
}

fn absolute_path_env(
    env_map: &BTreeMap<String, String>,
    key: &str,
    default_value: &str,
) -> Result<PathBuf, ExitError> {
    let value = optional_env(env_map, key).unwrap_or_else(|| default_value.to_string());
    let path = PathBuf::from(&value);
    if !path.is_absolute() {
        return Err(ExitError::new(
            2,
            format!("invalid_args: {key} must be an absolute path"),
        ));
    }
    Ok(path)
}

fn socket_addr_env(
    env_map: &BTreeMap<String, String>,
    key: &str,
    default_value: &str,
) -> Result<SocketAddr, ExitError> {
    let raw = optional_env(env_map, key).unwrap_or_else(|| default_value.to_string());
    raw.parse::<SocketAddr>()
        .map_err(|e| ExitError::new(2, format!("invalid_args: {key} parse failed: {e}")))
}

fn optional_port_env(
    env_map: &BTreeMap<String, String>,
    key: &str,
) -> Result<Option<u16>, ExitError> {
    let Some(raw) = optional_env(env_map, key) else {
        return Ok(None);
    };
    raw.parse::<u16>()
        .map(Some)
        .map_err(|e| ExitError::new(2, format!("invalid_args: {key} parse failed: {e}")))
}

fn validate_https_origin(origin: &str) -> Result<(), ExitError> {
    let url = Url::parse(origin)
        .map_err(|e| ExitError::new(2, format!("invalid_args: invalid url: {e}")))?;
    if url.scheme() != "https" {
        return Err(ExitError::new(
            2,
            "invalid_args: XP_API_BASE_URL must use https",
        ));
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err(ExitError::new(
            2,
            "invalid_args: XP_API_BASE_URL must not include a path",
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: XP_API_BASE_URL must not include query/fragment",
        ));
    }
    Ok(())
}

fn sanitize_label(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c);
        } else {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn validate_hostname(name: &str) -> Result<(), ExitError> {
    if name.len() > 253 {
        return Err(ExitError::new(2, "invalid_args: hostname is too long"));
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels.is_empty() {
        return Err(ExitError::new(2, "invalid_args: hostname is empty"));
    }
    for label in labels {
        if label.is_empty() || label.len() > 63 {
            return Err(ExitError::new(
                2,
                "invalid_args: hostname label length is invalid",
            ));
        }
        let bytes = label.as_bytes();
        if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
            return Err(ExitError::new(
                2,
                "invalid_args: hostname labels cannot start or end with '-'",
            ));
        }
        if !label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ExitError::new(
                2,
                "invalid_args: hostname must be lowercase dns labels",
            ));
        }
    }
    Ok(())
}

fn hostname_in_zone(hostname: &str, zone: &str) -> bool {
    hostname == zone || hostname.ends_with(&format!(".{zone}"))
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

async fn resolve_zone_from_hostname(
    api_base: &str,
    token: &str,
    account_id: &str,
    hostname: &str,
) -> Result<ZoneLookup, ExitError> {
    let candidates = zone_name_candidates(hostname);
    if candidates.is_empty() {
        return Err(ExitError::new(
            2,
            "invalid_args: XP_CLOUDFLARE_HOSTNAME is empty",
        ));
    }
    for candidate in candidates {
        let mut zones = cloudflare::find_zone_by_name(api_base, token, &candidate).await?;
        if zones.is_empty() {
            continue;
        }
        if !account_id.trim().is_empty() {
            zones.retain(|zone| zone.account_id.as_deref() == Some(account_id));
            if zones.is_empty() {
                continue;
            }
        }
        if zones.len() == 1 {
            return Ok(zones.remove(0));
        }
        return Err(ExitError::new(
            2,
            format!(
                "invalid_args: multiple Cloudflare zones matched {candidate}; set XP_CLOUDFLARE_ZONE_ID explicitly"
            ),
        ));
    }
    Err(ExitError::new(
        2,
        format!("invalid_args: unable to resolve Cloudflare zone for hostname {hostname}"),
    ))
}

fn render_dry_run(spec: &ContainerSpec, binaries: &BinaryPaths) {
    eprintln!("container run dry-run:");
    eprintln!("  - startup: {}", spec.startup.label());
    eprintln!("  - node_name: {}", spec.node_name);
    eprintln!("  - access_host: {}", spec.access_host);
    eprintln!("  - api_base_url: {}", spec.api_base_url);
    eprintln!(
        "  - node_meta_alignment: {}",
        if spec.node_meta_needs_realign {
            "realign"
        } else {
            "reuse"
        }
    );
    eprintln!("  - data_dir: {}", spec.data_dir.display());
    eprintln!("  - bind: {}", spec.bind);
    eprintln!("  - xray_api_addr: {}", spec.xray_api_addr);
    eprintln!("  - xp_bin: {}", binaries.xp.display());
    eprintln!("  - xray_bin: {}", binaries.xray.display());
    if let Some(cf) = spec.cloudflare.as_ref() {
        eprintln!("  - cloudflare: enabled");
        eprintln!("    - hostname: {}", cf.hostname);
        eprintln!("    - zone_id: {} ({})", cf.zone_id, cf.zone_name);
        eprintln!("    - tunnel_name: {}", cf.tunnel_name);
        eprintln!("    - origin_url: {}", cf.origin_url);
        eprintln!("    - token_source: {}", cf.token_source.display());
        eprintln!("    - cloudflared_bin: {}", binaries.cloudflared.display());
    } else {
        eprintln!("  - cloudflare: disabled");
    }
    if let Some(ddns) = spec.ddns.as_ref() {
        eprintln!("  - ddns: enabled");
        eprintln!("    - zone_id: {}", ddns.zone_id);
        eprintln!("    - token_file: {}", ddns.token_file.display());
    } else {
        eprintln!("  - ddns: disabled");
    }
    if let Some(vless) = spec.default_endpoints.vless.as_ref() {
        eprintln!("  - default_vless: enabled");
        eprintln!("    - port: {}", vless.port);
        eprintln!("    - dest: {}", vless.reality_dest);
        eprintln!("    - server_names: {}", vless.server_names.join(","));
        eprintln!("    - fingerprint: {}", vless.fingerprint);
    } else {
        eprintln!("  - default_vless: disabled");
    }
    if let Some(ss) = spec.default_endpoints.ss.as_ref() {
        eprintln!("  - default_ss: enabled");
        eprintln!("    - port: {}", ss.port);
    } else {
        eprintln!("  - default_ss: disabled");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::process::Command;

    fn env_map(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[tokio::test]
    async fn bootstrap_requires_admin_token() {
        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        let env = env_map(&[
            ("XP_NODE_NAME", "node-1"),
            ("XP_API_BASE_URL", "https://node-1.example.com"),
        ]);
        let err = ContainerSpec::from_env_map(&paths, &env, None)
            .await
            .unwrap_err();
        assert!(
            err.message
                .contains("bootstrap mode requires XP_ADMIN_TOKEN or XP_ADMIN_TOKEN_HASH")
        );
    }

    #[tokio::test]
    async fn bootstrap_derives_access_host_from_api_base_url() {
        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        let env = env_map(&[
            ("XP_NODE_NAME", "node-1"),
            ("XP_API_BASE_URL", "https://node-1.example.com"),
            ("XP_ADMIN_TOKEN", "secret"),
        ]);
        let spec = ContainerSpec::from_env_map(&paths, &env, None)
            .await
            .unwrap();
        assert_eq!(spec.access_host, "node-1.example.com");
        assert!(matches!(
            spec.startup,
            ContainerStartup::Bootstrap { needs_init: true }
        ));
        assert!(spec.bootstrap_admin_token_hash.is_some());
    }

    #[test]
    fn cloudflare_api_base_url_defaults_to_hostname() {
        let cf = ContainerCloudflare {
            account_id: "acc".to_string(),
            zone_id: "zone".to_string(),
            zone_name: "example.com".to_string(),
            hostname: "node-1.example.com".to_string(),
            tunnel_name: "xp-node-1".to_string(),
            origin_url: DEFAULT_CLOUDFLARE_ORIGIN_URL.to_string(),
            token: "token".to_string(),
            token_source: CloudflareTokenSource::Env,
        };
        let env = env_map(&[]);
        let api_base_url = resolve_api_base_url(&env, Some(&cf)).unwrap();
        let access_host = resolve_access_host(&env, &api_base_url, Some(&cf)).unwrap();
        assert_eq!(api_base_url, "https://node-1.example.com");
        assert_eq!(access_host, "node-1.example.com");
    }

    #[test]
    fn decodes_join_token_leader_api_base_url() {
        let token = crate::cluster_identity::JoinToken {
            cluster_id: "cluster".to_string(),
            leader_api_base_url: "https://leader.example.com".to_string(),
            cluster_ca_pem: "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n"
                .to_string(),
            token_id: "token-id".to_string(),
            one_time_secret: "secret".to_string(),
            expires_at: chrono::Utc::now(),
        }
        .encode_base64url_json();
        assert_eq!(
            decode_join_token_leader_api_base_url(&token).as_deref(),
            Some("https://leader.example.com")
        );
    }

    #[test]
    fn zone_candidates_walk_suffixes() {
        assert_eq!(
            zone_name_candidates("a.b.example.com"),
            vec!["a.b.example.com", "b.example.com", "example.com", "com"]
        );
    }

    #[test]
    fn detects_metadata_mismatch_without_blocking_reuse() {
        let meta = ClusterMetadata {
            schema_version: crate::cluster_metadata::CLUSTER_METADATA_SCHEMA_VERSION,
            cluster_id: "cluster".to_string(),
            node_id: "node-id".to_string(),
            node_name: "node-1".to_string(),
            access_host: "node-1.example.com".to_string(),
            api_base_url: "https://node-1.example.com".to_string(),
            has_cluster_ca_key: true,
            is_bootstrap_node: Some(true),
        };
        assert!(node_meta_mismatch(
            &meta,
            "node-2",
            "node-2.example.com",
            "https://node-2.example.com",
        ));
        let startup = resolve_startup(Some(&meta), None).unwrap();
        assert!(matches!(
            startup,
            ContainerStartup::Bootstrap { needs_init: false }
        ));
    }

    #[test]
    fn parses_default_endpoint_specs_from_env() {
        let env = env_map(&[
            ("XP_DEFAULT_VLESS_PORT", "53842"),
            ("XP_DEFAULT_VLESS_REALITY_DEST", "oneclient.sfx.ms:443"),
            (
                "XP_DEFAULT_VLESS_SERVER_NAMES",
                "oneclient.sfx.ms, skyapi.onedrive.com",
            ),
            ("XP_DEFAULT_SS_PORT", "53843"),
        ]);
        let spec = ManagedDefaultEndpointsSpec::from_env_map(&env).unwrap();
        assert_eq!(spec.vless.as_ref().unwrap().port, 53842);
        assert_eq!(
            spec.vless.as_ref().unwrap().server_names,
            vec!["oneclient.sfx.ms", "skyapi.onedrive.com"]
        );
        assert_eq!(spec.vless.as_ref().unwrap().fingerprint, "chrome");
        assert_eq!(spec.ss.as_ref().unwrap().port, 53843);
    }

    #[test]
    fn vless_reconcile_preserves_keys_and_updates_reality_settings() {
        let endpoint = DefaultVlessEndpointSpec {
            port: 53842,
            reality_dest: "oneclient.sfx.ms:443".to_string(),
            server_names: vec![
                "oneclient.sfx.ms".to_string(),
                "skyapi.onedrive.com".to_string(),
            ],
            fingerprint: "chrome".to_string(),
        }
        .create_new("node-id".to_string())
        .unwrap();

        let desired = DefaultVlessEndpointSpec {
            port: 60000,
            reality_dest: "download.example.com:443".to_string(),
            server_names: vec!["download.example.com".to_string()],
            fingerprint: "firefox".to_string(),
        };
        let updated = desired.reconcile_existing(&endpoint).unwrap();
        let old_meta: VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(endpoint.meta.clone()).unwrap();
        let new_meta: VlessRealityVisionTcpEndpointMeta =
            serde_json::from_value(updated.meta.clone()).unwrap();
        assert_eq!(updated.port, 60000);
        assert_eq!(new_meta.reality.dest, "download.example.com:443");
        assert_eq!(new_meta.reality.server_names, vec!["download.example.com"]);
        assert_eq!(new_meta.reality.fingerprint, "firefox");
        assert_eq!(new_meta.reality_keys, old_meta.reality_keys);
        assert_eq!(new_meta.short_ids, old_meta.short_ids);
        assert_eq!(new_meta.active_short_id, old_meta.active_short_id);
    }

    #[tokio::test]
    async fn child_startup_wait_accepts_long_running_process() {
        let mut child = Command::new("sh").args(["-c", "sleep 1"]).spawn().unwrap();
        wait_for_child_startup("test-child", &mut child, Duration::from_millis(50))
            .await
            .unwrap();
        cleanup_child(&mut child).await;
    }

    #[tokio::test]
    async fn child_startup_wait_rejects_early_exit() {
        let mut child = Command::new("sh").args(["-c", "exit 23"]).spawn().unwrap();
        let err = wait_for_child_startup("test-child", &mut child, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(
            err.message
                .contains("container_failed: test-child exited with code 23")
        );
    }
}
