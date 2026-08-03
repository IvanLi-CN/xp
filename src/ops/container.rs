use crate::admin_token::{
    hash_admin_token_argon2id, is_default_admin_token_hash_profile, parse_admin_token_hash,
};
use crate::cluster_metadata::{ClusterMetadata, ClusterPaths};
use crate::managed_default_endpoints::{
    DefaultSsEndpointSpec, DefaultVlessEndpointSpec, ManagedDefaultEndpointsSpec,
    build_default_ss_endpoint_spec, build_default_vless_endpoint_spec,
};
use crate::ops::cli::{CloudflareProvisionArgs, ContainerRunArgs, ExitError};
use crate::ops::cloudflare::{self, CloudflareTokenSource, ZoneLookup};
use crate::ops::init;
use crate::ops::paths::Paths;
use crate::ops::util::{Mode, ensure_dir, write_string_if_changed};
use futures_util::future::pending;
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

mod admin_token_sync;
#[path = "container_managed_default.rs"]
mod container_managed_default;
mod runtime_env;
use admin_token_sync::reconcile_configured_admin_token_hash;
use runtime_env::build_runtime_env;

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

#[derive(Debug, Clone)]
struct ContainerSpec {
    node_name: String,
    access_host: String,
    api_base_url: String,
    data_dir: PathBuf,
    bind: SocketAddr,
    xray_api_addr: SocketAddr,
    startup: ContainerStartup,
    configured_admin_token_hash: Option<String>,
    cloudflare: Option<ContainerCloudflare>,
    ddns: Option<ContainerDdns>,
    vless_canary_token: Option<String>,
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
    cutover::write_marker_if(
        &spec.data_dir,
        args.allow_internal_auth_v2_cutover && mode == Mode::Real,
    )?;

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
        let mut child = spawn_cloudflared(&binaries.cloudflared, &spec.runtime_env)?;
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

    if let Err(err) = reconcile_configured_admin_token_hash(&paths, &spec) {
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

    let mut xray_child = match spawn_xray(&binaries.xray, &paths, &spec.runtime_env) {
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
        match spawn_cloudflared(&binaries.cloudflared, &spec.runtime_env) {
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
        let join_leader_api_base_url = if existing_meta.is_none() {
            join_token
                .as_deref()
                .and_then(decode_join_token_leader_api_base_url)
        } else {
            None
        };
        let cloudflare_enabled = bool_env(env_map, "XP_ENABLE_CLOUDFLARE", false)?;

        let cloudflare = if cloudflare_enabled {
            Some(build_cloudflare_spec(paths, env_map, &node_name).await?)
        } else {
            None
        };

        let api_base_url = resolve_api_base_url(env_map, cloudflare.as_ref())?;
        let access_host = resolve_access_host(env_map, &api_base_url, cloudflare.as_ref())?;
        let ddns = build_ddns_spec(paths, env_map, &access_host, cloudflare.as_ref()).await?;
        let default_endpoints = ManagedDefaultEndpointsSpec::from_env_map(env_map, &access_host)?;
        let runtime_env = build_runtime_env(env_map, ddns.as_ref());
        let vless_canary_token = load_vless_canary_runtime_token(
            paths,
            &default_endpoints,
            &runtime_env,
            cloudflare.as_ref(),
            ddns.as_ref(),
        )?;

        let startup = resolve_startup(existing_meta, join_token.as_deref())?;
        let node_meta_needs_realign = existing_meta
            .is_some_and(|meta| node_meta_mismatch(meta, &node_name, &access_host, &api_base_url));

        let configured_admin_token_hash = if startup.requires_bootstrap_token() {
            Some(resolve_bootstrap_admin_token_hash(env_map)?)
        } else {
            resolve_configured_admin_token_hash(env_map)?
        };

        Ok(Self {
            node_name,
            access_host,
            api_base_url,
            data_dir,
            bind,
            xray_api_addr,
            startup,
            configured_admin_token_hash,
            cloudflare,
            ddns,
            vless_canary_token,
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
    fn from_env_map(
        env_map: &BTreeMap<String, String>,
        access_host: &str,
    ) -> Result<Self, ExitError> {
        Ok(Self {
            vless: parse_default_vless_endpoint_spec(env_map, access_host)?,
            ss: parse_default_ss_endpoint_spec(env_map)?,
        })
    }
}

fn parse_default_vless_endpoint_spec(
    env_map: &BTreeMap<String, String>,
    access_host: &str,
) -> Result<Option<DefaultVlessEndpointSpec>, ExitError> {
    let port = optional_port_env(env_map, "XP_DEFAULT_VLESS_PORT")?;
    let server_names_raw = optional_env(env_map, "XP_DEFAULT_VLESS_SERVER_NAMES");
    let fingerprint = optional_env(env_map, "XP_DEFAULT_VLESS_FINGERPRINT");
    let vless_canary_bind = socket_addr_env(
        env_map,
        "XP_VLESS_CANARY_BIND",
        crate::config::DEFAULT_VLESS_CANARY_BIND,
    )?;

    build_default_vless_endpoint_spec(
        port,
        access_host,
        server_names_raw.as_deref(),
        fingerprint.as_deref(),
        vless_canary_bind,
    )
    .map_err(|err| ExitError::new(2, format!("invalid_args: {err}")))
}

fn parse_default_ss_endpoint_spec(
    env_map: &BTreeMap<String, String>,
) -> Result<Option<DefaultSsEndpointSpec>, ExitError> {
    build_default_ss_endpoint_spec(optional_port_env(env_map, "XP_DEFAULT_SS_PORT")?)
        .map_err(|err| ExitError::new(2, format!("invalid_args: {err}")))
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
    if let Some(hash) = resolve_configured_admin_token_hash(env_map)? {
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

fn resolve_configured_admin_token_hash(
    env_map: &BTreeMap<String, String>,
) -> Result<Option<String>, ExitError> {
    let Some(hash) = optional_env(env_map, "XP_ADMIN_TOKEN_HASH") else {
        return Ok(None);
    };
    let Some(parsed) = parse_admin_token_hash(&hash) else {
        return Err(ExitError::new(
            2,
            "invalid_args: XP_ADMIN_TOKEN_HASH is present but invalid",
        ));
    };
    if !is_default_admin_token_hash_profile(&parsed) {
        return Err(ExitError::new(
            2,
            "invalid_args: XP_ADMIN_TOKEN_HASH must use argon2id m=4096,t=3,p=1",
        ));
    }
    Ok(Some(parsed.as_str().to_string()))
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
            .configured_admin_token_hash
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
    if let Some(token) = spec.vless_canary_token.as_deref() {
        let token_file = spec
            .runtime_env
            .get("XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE));
        ensure_xp_runtime_token_file(paths, &token_file, mode, token)?;
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
    write_runtime_token_file(&target, &ddns.token, mode)
}

fn ensure_xp_runtime_token_file(
    paths: &Paths,
    token_file: &Path,
    mode: Mode,
    token: &str,
) -> Result<(), ExitError> {
    let target = paths.map_abs(token_file);
    write_runtime_token_file(&target, token, mode)
}

fn write_runtime_token_file(target: &Path, token: &str, mode: Mode) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would write: {}", target.display());
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        ensure_dir(parent).map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    }
    write_string_if_changed(target, &(token.trim().to_string() + "\n"))
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    Ok(())
}

fn load_vless_canary_runtime_token(
    paths: &Paths,
    default_endpoints: &ManagedDefaultEndpointsSpec,
    runtime_env: &BTreeMap<String, String>,
    cloudflare: Option<&ContainerCloudflare>,
    ddns: Option<&ContainerDdns>,
) -> Result<Option<String>, ExitError> {
    if default_endpoints.vless.is_none() {
        return Ok(None);
    }
    if let Some(ddns) = ddns {
        return Ok(Some(ddns.token.clone()));
    }
    if let Some(cloudflare) = cloudflare {
        return Ok(Some(cloudflare.token.clone()));
    }
    let token_file = runtime_env
        .get("XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(crate::config::DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE));
    let token_path = paths.map_abs(&token_file);
    if token_path.exists() {
        return crate::vless_https_canary::read_cloudflare_token_from_file(&token_path)
            .map(Some)
            .map_err(|err| {
                ExitError::new(
                    2,
                    format!(
                        "cloudflare token error: read vless canary token file {}: {err}",
                        token_path.display()
                    ),
                )
            });
    }
    cloudflare::load_cloudflare_token_for_deploy(paths, None, None)
        .map(|(token, _)| Some(token))
        .map_err(|err| {
            if err.message == "token_missing" {
                ExitError::new(
                    2,
                    "cloudflare_token_missing: managed default VLESS canary requires CLOUDFLARE_API_TOKEN or persisted tunnel token state",
                )
            } else {
                ExitError::new(2, format!("cloudflare token error: {}", err.message))
            }
        })
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
    let xp_dir = paths.etc_xp_dir();
    let xray_dir = paths.etc_xray_dir();
    let cloudflared_dir = paths.etc_cloudflared_dir();
    let xp_ops_cloudflare_dir = paths.etc_xp_ops_cloudflare_dir();
    let dirs = [
        data_dir.as_path(),
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
            migrate_existing_tunnel: false,
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
            &spec.default_endpoints,
            socket_addr_env(
                &spec.runtime_env,
                "XP_VLESS_CANARY_BIND",
                crate::config::DEFAULT_VLESS_CANARY_BIND,
            )?,
            control_plane_base_url,
            Mode::Real,
        )
        .await?;
    }

    container_managed_default::reconcile(paths, spec, control_plane_base_url).await
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

fn spawn_xray(
    xray_bin: &Path,
    paths: &Paths,
    runtime_env: &BTreeMap<String, String>,
) -> Result<Child, ExitError> {
    Command::new(xray_bin)
        .arg("run")
        .arg("-c")
        .arg(paths.etc_xray_config())
        .env(
            "GOMEMLIMIT",
            runtime_env
                .get("XP_XRAY_GOMEMLIMIT")
                .map(String::as_str)
                .unwrap_or("16MiB"),
        )
        .env(
            "GOGC",
            runtime_env
                .get("XP_XRAY_GOGC")
                .map(String::as_str)
                .unwrap_or("50"),
        )
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| ExitError::new(6, format!("container_start_failed: spawn xray: {e}")))
}

fn spawn_cloudflared(
    cloudflared_bin: &Path,
    runtime_env: &BTreeMap<String, String>,
) -> Result<Child, ExitError> {
    Command::new(cloudflared_bin)
        .arg("--no-autoupdate")
        .arg("--protocol")
        .arg(cloudflared_protocol(runtime_env))
        .arg("--config")
        .arg("/etc/cloudflared/config.yml")
        .arg("tunnel")
        .arg("run")
        .env(
            "GOMEMLIMIT",
            runtime_env
                .get("XP_CLOUDFLARED_GOMEMLIMIT")
                .map(String::as_str)
                .unwrap_or("8MiB"),
        )
        .env(
            "GOGC",
            runtime_env
                .get("XP_CLOUDFLARED_GOGC")
                .map(String::as_str)
                .unwrap_or("50"),
        )
        .env(
            "TUNNEL_MANAGEMENT_DIAGNOSTICS",
            runtime_env
                .get("XP_CLOUDFLARED_MANAGEMENT_DIAGNOSTICS")
                .map(String::as_str)
                .unwrap_or("false"),
        )
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| ExitError::new(6, format!("container_start_failed: spawn cloudflared: {e}")))
}

fn cloudflared_protocol(runtime_env: &BTreeMap<String, String>) -> &str {
    runtime_env
        .get("XP_CLOUDFLARED_PROTOCOL")
        .map(String::as_str)
        .unwrap_or("http2")
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

mod cutover;

#[cfg(test)]
mod tests;
