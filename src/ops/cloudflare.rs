use crate::ops::cli::{CloudflareProvisionArgs, CloudflareTokenSetArgs, ExitError};
use crate::ops::install;
use crate::ops::paths::Paths;
use crate::ops::platform::{Distro, InitSystem, detect_distro, detect_init_system};
use crate::ops::util::{
    Mode, chmod, ensure_dir, is_executable, is_test_root, write_bytes_if_changed,
    write_string_if_changed,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::net::IpAddr;
use std::path::Path;
use std::process::Command;

mod cloudflare_provision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudflareTokenSource {
    Flag,
    Stdin,
    Env,
    File,
}

impl CloudflareTokenSource {
    pub fn display(&self) -> &'static str {
        match self {
            CloudflareTokenSource::Flag => "flag",
            CloudflareTokenSource::Stdin => "stdin",
            CloudflareTokenSource::Env => "env",
            CloudflareTokenSource::File => "file",
        }
    }
}

pub async fn cmd_cloudflare_token_set(
    paths: Paths,
    args: CloudflareTokenSetArgs,
) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };
    let token = read_token_input(&args)?;
    set_token_value(&paths, &token, mode)?;
    Ok(())
}

pub fn set_token_value(paths: &Paths, token: &str, mode: Mode) -> Result<(), ExitError> {
    if token.trim().is_empty() {
        return Err(ExitError::new(2, "invalid_args: token is empty"));
    }

    let token_path = paths.etc_xp_ops_cloudflare_token();
    if mode == Mode::DryRun {
        eprintln!("would write token to: {}", token_path.display());
        return Ok(());
    }

    ensure_dir(&paths.etc_xp_ops_cloudflare_dir())
        .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
    fs::write(&token_path, token.trim().as_bytes())
        .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
    chmod(&token_path, 0o600).ok();
    Ok(())
}

pub async fn cmd_cloudflare_provision(
    paths: Paths,
    args: CloudflareProvisionArgs,
) -> Result<(), ExitError> {
    let token = load_cloudflare_token(&paths).map_err(|e| ExitError::new(3, e))?;
    cmd_cloudflare_provision_with_token(paths, args, token).await
}

pub async fn cmd_cloudflare_provision_with_token(
    paths: Paths,
    args: CloudflareProvisionArgs,
    token: String,
) -> Result<(), ExitError> {
    cloudflare_provision::run(
        paths,
        args,
        token,
        cloudflare_provision::ProvisionRuntime::ManagedService,
    )
    .await
}

pub async fn cmd_cloudflare_provision_container(
    paths: Paths,
    args: CloudflareProvisionArgs,
    token: String,
) -> Result<(), ExitError> {
    cloudflare_provision::run(
        paths,
        args,
        token,
        cloudflare_provision::ProvisionRuntime::Container,
    )
    .await
}

fn read_token_input(args: &CloudflareTokenSetArgs) -> Result<String, ExitError> {
    if args.from_stdin == args.from_env.is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: specify exactly one of --from-stdin or --from-env",
        ));
    }

    if let Some(name) = &args.from_env {
        return std::env::var(name)
            .map_err(|_| ExitError::new(2, format!("invalid_args: env {name} is not set")));
    }

    let mut s = String::new();
    io::stdin()
        .read_to_string(&mut s)
        .map_err(|e| ExitError::new(2, format!("invalid_args: read stdin: {e}")))?;
    Ok(s.trim().to_string())
}

fn load_cloudflare_token(paths: &Paths) -> Result<String, String> {
    if let Ok(v) = std::env::var("CLOUDFLARE_API_TOKEN")
        && !v.trim().is_empty()
    {
        return Ok(v);
    }

    let p = paths.etc_xp_ops_cloudflare_token();
    let v = fs::read_to_string(&p).map_err(|_| "token_missing".to_string())?;
    if v.trim().is_empty() {
        return Err("token_missing".to_string());
    }
    Ok(v.trim().to_string())
}

pub fn load_cloudflare_token_for_deploy(
    paths: &Paths,
    token_from_flag: Option<&str>,
    token_from_stdin: Option<&str>,
) -> Result<(String, CloudflareTokenSource), ExitError> {
    if let Some(v) = token_from_flag {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return Ok((trimmed.to_string(), CloudflareTokenSource::Flag));
        }
    }

    if let Some(v) = token_from_stdin {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return Ok((trimmed.to_string(), CloudflareTokenSource::Stdin));
        }
    }

    if let Ok(v) = std::env::var("CLOUDFLARE_API_TOKEN")
        && !v.trim().is_empty()
    {
        return Ok((v, CloudflareTokenSource::Env));
    }

    let p = paths.etc_xp_ops_cloudflare_token();
    let v = fs::read_to_string(&p).map_err(|_| ExitError::new(3, "token_missing"))?;
    let trimmed = v.trim();
    if trimmed.is_empty() {
        return Err(ExitError::new(3, "token_missing"));
    }
    Ok((trimmed.to_string(), CloudflareTokenSource::File))
}

async fn ensure_cloudflared_present(
    paths: &Paths,
    distro: Distro,
    mode: Mode,
) -> Result<(), ExitError> {
    let bin = cloudflared_binary_path(paths, distro);
    if bin.exists() && is_executable(&bin) {
        return Ok(());
    }

    // Install on demand using the same fixed strategy as `xp-ops install`.
    let install_args = crate::ops::cli::InstallArgs {
        only: Some(crate::ops::cli::InstallOnly::Cloudflared),
        xray_version: "latest".to_string(),
        dry_run: mode == Mode::DryRun,
    };
    install::cmd_install(paths.clone(), install_args).await?;
    Ok(())
}

pub(super) fn cloudflared_binary_path(paths: &Paths, distro: Distro) -> std::path::PathBuf {
    let bin_abs = match distro {
        Distro::Arch | Distro::Debian | Distro::Rhel => Path::new("/usr/bin/cloudflared"),
        Distro::Alpine => Path::new("/usr/local/bin/cloudflared"),
    };
    paths.map_abs(bin_abs)
}

pub(super) fn ensure_cloudflared_service(
    paths: &Paths,
    distro: Distro,
    init_system: InitSystem,
    mode: Mode,
) -> Result<bool, ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would ensure cloudflared service files exist");
        return Ok(false);
    }

    if !is_test_root(paths.root()) {
        // Ensure runtime user/group exists.
        let _ = match distro {
            Distro::Alpine => {
                let _ = Command::new("addgroup")
                    .args(["-S", "cloudflared"])
                    .status();
                Command::new("adduser")
                    .args([
                        "-S",
                        "-D",
                        "-H",
                        "-s",
                        "/sbin/nologin",
                        "-G",
                        "cloudflared",
                        "cloudflared",
                    ])
                    .status()
            }
            Distro::Arch | Distro::Debian | Distro::Rhel => Command::new("useradd")
                .args([
                    "--system",
                    "--home",
                    "/var/lib/cloudflared",
                    "--shell",
                    "/usr/sbin/nologin",
                    "--user-group",
                    "cloudflared",
                ])
                .status(),
        };
    }

    let changed = match init_system {
        InitSystem::Systemd => {
            let dir = paths.systemd_unit_dir();
            ensure_dir(&dir).map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            let unit_path = dir.join("cloudflared.service");
            let unit = systemd_cloudflared_unit();
            write_string_if_changed(&unit_path, &unit)
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?
        }
        InitSystem::OpenRc => {
            let initd = paths.openrc_initd_dir();
            ensure_dir(&initd).map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            let script_path = initd.join("cloudflared");
            let script = openrc_cloudflared_script();
            let changed = write_string_if_changed(&script_path, &script)
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            chmod(&script_path, 0o755).ok();
            changed
        }
        InitSystem::None => false,
    };

    Ok(changed)
}

fn enable_cloudflared_service(
    init_system: InitSystem,
    mode: Mode,
    paths: &Paths,
) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        return Ok(());
    }
    if is_test_root(paths.root()) {
        return Ok(());
    }
    match init_system {
        InitSystem::Systemd => {
            Command::new("systemctl")
                .args(["daemon-reload"])
                .status()
                .ok();
            let status = Command::new("systemctl")
                .args(["enable", "--now", "cloudflared.service"])
                .status()
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            if !status.success() {
                return Err(ExitError::new(
                    6,
                    "filesystem_error: enable cloudflared failed",
                ));
            }
        }
        InitSystem::OpenRc => {
            let enable_status = Command::new("rc-update")
                .args(["add", "cloudflared", "default"])
                .status()
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            if !enable_status.success() {
                return Err(ExitError::new(
                    6,
                    "filesystem_error: enable cloudflared failed",
                ));
            }
            let start_status = Command::new("rc-service")
                .args(["cloudflared", "start"])
                .status()
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            if !start_status.success() {
                return Err(ExitError::new(
                    6,
                    "filesystem_error: start cloudflared failed",
                ));
            }
        }
        InitSystem::None => {}
    }
    Ok(())
}

fn ensure_cloudflared_file_ownership(
    paths: &Paths,
    tunnel_id: &str,
    mode: Mode,
    runtime: cloudflare_provision::ProvisionRuntime,
) -> Result<(), ExitError> {
    if mode == Mode::DryRun
        || is_test_root(paths.root())
        || runtime == cloudflare_provision::ProvisionRuntime::Container
    {
        return Ok(());
    }
    let config = paths.etc_cloudflared_config();
    let cred = paths
        .etc_cloudflared_dir()
        .join(format!("{tunnel_id}.json"));
    for p in [config, cred] {
        let path = p.display().to_string();
        let status = Command::new("chown")
            .args(["cloudflared:cloudflared", path.as_str()])
            .status()
            .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
        if !status.success() {
            return Err(ExitError::new(
                6,
                format!("filesystem_error: chown {}", p.display()),
            ));
        }
    }
    Ok(())
}

fn systemd_cloudflared_unit() -> String {
    "[Unit]\n\
Description=cloudflared (Cloudflare Tunnel)\n\
Wants=network-online.target\n\
After=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
User=cloudflared\n\
Group=cloudflared\n\
Environment=GOMEMLIMIT=8MiB\n\
Environment=GOGC=50\n\
Environment=TUNNEL_MANAGEMENT_DIAGNOSTICS=false\n\
Environment=XP_CLOUDFLARED_PROTOCOL=http2\n\
ExecStart=/usr/bin/cloudflared --no-autoupdate --protocol ${XP_CLOUDFLARED_PROTOCOL} --config /etc/cloudflared/config.yml tunnel run\n\
Restart=always\n\
RestartSec=2s\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n"
        .to_string()
}

fn openrc_cloudflared_script() -> String {
    r#"#!/sbin/openrc-run

name="cloudflared"
description="cloudflared (Cloudflare Tunnel)"

command="/usr/local/bin/cloudflared"
command_args="--no-autoupdate --protocol ${XP_CLOUDFLARED_PROTOCOL:-http2} --config /etc/cloudflared/config.yml tunnel run"
command_user="cloudflared:cloudflared"
export GOMEMLIMIT="${GOMEMLIMIT:-8MiB}"
export GOGC="${GOGC:-50}"
export TUNNEL_MANAGEMENT_DIAGNOSTICS="${TUNNEL_MANAGEMENT_DIAGNOSTICS:-false}"

# Ensure automatic recovery on crashes without busy-looping.
supervisor=supervise-daemon
respawn_delay=2
respawn_max=0

depend() {
  need net
}
"#
    .to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Settings {
    enabled: bool,
    install_mode: String,
    origin_url: String,
    account_id: String,
    zone_id: String,
    hostname: String,
    tunnel_id: Option<String>,
    dns_record_id: Option<String>,
}

fn save_settings(paths: &Paths, s: &Settings) -> Result<(), ExitError> {
    ensure_dir(&paths.etc_xp_ops_cloudflare_dir())
        .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
    let content = serde_json::to_string_pretty(s)
        .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
    write_string_if_changed(&paths.etc_xp_ops_cloudflare_settings(), &(content + "\n"))
        .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
    chmod(&paths.etc_xp_ops_cloudflare_settings(), 0o640).ok();
    Ok(())
}

#[derive(Debug)]
pub(crate) struct CloudflareClient {
    base: String,
    token: String,
    client: reqwest::Client,
}

impl CloudflareClient {
    pub(crate) fn new(base: String, token: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("xp-ops")
            .build()
            .expect("reqwest client");
        Self {
            base,
            token,
            client,
        }
    }

    async fn create_tunnel(
        &self,
        account_id: &str,
        name: &str,
    ) -> anyhow::Result<CreateTunnelResult> {
        let url = format!(
            "{}/client/v4/accounts/{account_id}/cfd_tunnel",
            self.base.trim_end_matches('/')
        );
        let body = serde_json::json!({ "name": name, "config_src": "cloudflare" });
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        parse_cloudflare_response::<CreateTunnelResult>(resp).await
    }

    async fn delete_tunnel(&self, account_id: &str, tunnel_id: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}",
            self.base.trim_end_matches('/')
        );
        let resp = self
            .client
            .delete(url)
            .bearer_auth(&self.token)
            .send()
            .await?;
        let _ = parse_cloudflare_response::<serde_json::Value>(resp).await?;
        Ok(())
    }

    async fn get_tunnel_config(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!(
            "{}/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations",
            self.base.trim_end_matches('/')
        );
        let resp = self.client.get(url).bearer_auth(&self.token).send().await?;
        parse_cloudflare_response::<serde_json::Value>(resp).await
    }

    async fn put_tunnel_config(
        &self,
        account_id: &str,
        tunnel_id: &str,
        config: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations",
            self.base.trim_end_matches('/')
        );
        let resp = self
            .client
            .put(url)
            .bearer_auth(&self.token)
            .json(config)
            .send()
            .await?;
        let _ = parse_cloudflare_response::<serde_json::Value>(resp).await?;
        Ok(())
    }

    async fn create_dns_record(
        &self,
        zone_id: &str,
        hostname: &str,
        tunnel_id: &str,
    ) -> anyhow::Result<String> {
        let url = format!(
            "{}/client/v4/zones/{zone_id}/dns_records",
            self.base.trim_end_matches('/')
        );
        let content = format!("{tunnel_id}.cfargotunnel.com");
        let body = serde_json::json!({
          "type": "CNAME",
          "name": hostname,
          "content": content,
          "proxied": true
        });
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        Ok(parse_cloudflare_response::<DnsRecordResult>(resp).await?.id)
    }

    async fn patch_dns_record(
        &self,
        zone_id: &str,
        dns_record_id: &str,
        _hostname: &str,
        tunnel_id: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/client/v4/zones/{zone_id}/dns_records/{dns_record_id}",
            self.base.trim_end_matches('/')
        );
        let content = format!("{tunnel_id}.cfargotunnel.com");
        // PATCH only the owned CNAME target. Cloudflare keeps unrelated record
        // attributes such as TTL, proxied mode, comments, and tags intact.
        let body = serde_json::json!({ "content": content });
        let resp = self
            .client
            .patch(url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        let _ = parse_cloudflare_response::<serde_json::Value>(resp).await?;
        Ok(())
    }

    async fn patch_dns_content(
        &self,
        zone_id: &str,
        dns_record_id: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/client/v4/zones/{zone_id}/dns_records/{dns_record_id}",
            self.base.trim_end_matches('/')
        );
        let resp = self
            .client
            .patch(url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "content": content }))
            .send()
            .await?;
        let _ = parse_cloudflare_response::<serde_json::Value>(resp).await?;
        Ok(())
    }

    async fn get_zone(&self, zone_id: &str) -> anyhow::Result<ZoneResult> {
        let url = format!(
            "{}/client/v4/zones/{zone_id}",
            self.base.trim_end_matches('/')
        );
        let resp = self.client.get(url).bearer_auth(&self.token).send().await?;
        parse_cloudflare_response::<ZoneResult>(resp).await
    }

    async fn list_dns_records(
        &self,
        zone_id: &str,
        hostname: &str,
    ) -> anyhow::Result<Vec<DnsRecordInfo>> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/client/v4/zones/{zone_id}/dns_records",
            self.base.trim_end_matches('/')
        ))?;
        url.query_pairs_mut().append_pair("name", hostname);
        let resp = self.client.get(url).bearer_auth(&self.token).send().await?;
        parse_cloudflare_response::<Vec<DnsRecordInfo>>(resp).await
    }

    pub(crate) async fn list_zones_by_name(&self, name: &str) -> anyhow::Result<Vec<ZoneLookup>> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/client/v4/zones",
            self.base.trim_end_matches('/')
        ))?;
        url.query_pairs_mut().append_pair("name", name);
        let resp = self.client.get(url).bearer_auth(&self.token).send().await?;
        let zones = parse_cloudflare_response::<Vec<ZoneListResult>>(resp).await?;
        Ok(zones
            .into_iter()
            .map(|z| ZoneLookup {
                id: z.id,
                name: z.name,
                account_id: z.account.id,
            })
            .collect())
    }

    async fn list_tunnels(&self, account_id: &str) -> anyhow::Result<Vec<TunnelInfo>> {
        let url = format!(
            "{}/client/v4/accounts/{account_id}/cfd_tunnel",
            self.base.trim_end_matches('/')
        );
        let resp = self.client.get(url).bearer_auth(&self.token).send().await?;
        let tunnels = parse_cloudflare_response::<Vec<TunnelResult>>(resp).await?;
        Ok(tunnels
            .into_iter()
            .map(|t| TunnelInfo {
                id: t.id,
                name: t.name,
            })
            .collect())
    }

    pub(crate) async fn list_dns_records_by_type(
        &self,
        zone_id: &str,
        hostname: &str,
        record_type: &str,
    ) -> anyhow::Result<Vec<DnsRecordInfo>> {
        let records = self.list_dns_records(zone_id, hostname).await?;
        Ok(records
            .into_iter()
            .filter(|record| record.record_type.eq_ignore_ascii_case(record_type))
            .collect())
    }

    pub(crate) async fn create_ip_dns_record(
        &self,
        zone_id: &str,
        hostname: &str,
        ip: IpAddr,
        proxied: bool,
        ttl: u32,
    ) -> anyhow::Result<DnsRecordInfo> {
        let url = format!(
            "{}/client/v4/zones/{zone_id}/dns_records",
            self.base.trim_end_matches('/')
        );
        let body = ip_dns_record_body(hostname, ip, proxied, ttl);
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        parse_cloudflare_response::<DnsRecordInfo>(resp).await
    }

    pub(crate) async fn patch_ip_dns_record(
        &self,
        zone_id: &str,
        dns_record_id: &str,
        hostname: &str,
        ip: IpAddr,
        proxied: bool,
        ttl: u32,
    ) -> anyhow::Result<DnsRecordInfo> {
        let url = format!(
            "{}/client/v4/zones/{zone_id}/dns_records/{dns_record_id}",
            self.base.trim_end_matches('/')
        );
        let body = ip_dns_record_body(hostname, ip, proxied, ttl);
        let resp = self
            .client
            .patch(url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        parse_cloudflare_response::<DnsRecordInfo>(resp).await
    }

    pub(crate) async fn delete_dns_record(
        &self,
        zone_id: &str,
        dns_record_id: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/client/v4/zones/{zone_id}/dns_records/{dns_record_id}",
            self.base.trim_end_matches('/')
        );
        let resp = self
            .client
            .delete(url)
            .bearer_auth(&self.token)
            .send()
            .await?;
        let _ = parse_cloudflare_response::<serde_json::Value>(resp).await?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct CloudflareResponse<T> {
    success: bool,
    errors: Vec<CloudflareApiError>,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct CloudflareApiError {
    code: Option<i64>,
    message: Option<String>,
}

impl<T> CloudflareResponse<T> {
    fn into_result(self, status: reqwest::StatusCode) -> anyhow::Result<T> {
        if self.success {
            return self.result.ok_or_else(|| anyhow::anyhow!("missing result"));
        }
        let msg = format_cloudflare_errors(self.errors);
        anyhow::bail!("cloudflare error (status {status}): {msg}")
    }
}

#[derive(Debug, Deserialize)]
struct CreateTunnelResult {
    id: String,
    credentials_file: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct DnsRecordResult {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ZoneAccount {
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ZoneResult {
    name: String,
    account: ZoneAccount,
}

#[derive(Debug, Deserialize)]
struct TunnelResult {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ZoneListResult {
    id: String,
    name: String,
    account: ZoneAccount,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsRecordInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub name: String,
    pub content: String,
    pub proxied: Option<bool>,
    pub ttl: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ZoneInfo {
    pub name: String,
    pub account_id: Option<String>,
}

fn ip_dns_record_body(hostname: &str, ip: IpAddr, proxied: bool, ttl: u32) -> serde_json::Value {
    serde_json::json!({
      "type": match ip {
        IpAddr::V4(_) => "A",
        IpAddr::V6(_) => "AAAA",
      },
      "name": hostname,
      "content": ip.to_string(),
      "proxied": proxied,
      "ttl": ttl,
    })
}

#[derive(Debug, Clone)]
pub struct TunnelInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ZoneLookup {
    pub id: String,
    pub name: String,
    pub account_id: Option<String>,
}

fn format_cloudflare_errors(errors: Vec<CloudflareApiError>) -> String {
    if errors.is_empty() {
        return "unknown".to_string();
    }
    let mut msgs = Vec::new();
    for e in errors {
        let msg = match (e.code, e.message) {
            (Some(81053), Some(m)) => format!(
                "81053:{m} (hint: a record with this hostname already exists; delete the existing A/AAAA/CNAME or choose a different hostname)"
            ),
            (Some(c), Some(m)) => format!("{c}:{m}"),
            (Some(c), None) => format!("{c}"),
            (None, Some(m)) => m,
            (None, None) => "unknown".to_string(),
        };
        msgs.push(msg);
    }
    msgs.join(", ")
}

async fn parse_cloudflare_response<T: DeserializeOwned>(
    resp: reqwest::Response,
) -> anyhow::Result<T> {
    let status = resp.status();
    let text = resp.text().await?;
    let api: CloudflareResponse<T> = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("cloudflare invalid json (status {status}): {e}"))?;
    api.into_result(status)
}

pub fn cloudflare_api_base() -> String {
    std::env::var("CLOUDFLARE_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.cloudflare.com".to_string())
}

pub async fn fetch_zone_info(
    api_base: &str,
    token: &str,
    zone_id: &str,
) -> Result<ZoneInfo, ExitError> {
    let client = CloudflareClient::new(api_base.to_string(), token.to_string());
    let zone = client
        .get_zone(zone_id)
        .await
        .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))?;
    Ok(ZoneInfo {
        name: zone.name,
        account_id: zone.account.id,
    })
}

pub async fn find_dns_record(
    api_base: &str,
    token: &str,
    zone_id: &str,
    hostname: &str,
) -> Result<Option<DnsRecordInfo>, ExitError> {
    let client = CloudflareClient::new(api_base.to_string(), token.to_string());
    let records = client
        .list_dns_records(zone_id, hostname)
        .await
        .map_err(|e| ExitError::new(5, format!("dns_error: {e}")))?;
    Ok(records.into_iter().next())
}

pub async fn find_zone_by_name(
    api_base: &str,
    token: &str,
    name: &str,
) -> Result<Vec<ZoneLookup>, ExitError> {
    let client = CloudflareClient::new(api_base.to_string(), token.to_string());
    client
        .list_zones_by_name(name)
        .await
        .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))
}

pub async fn find_tunnel_by_name(
    api_base: &str,
    token: &str,
    account_id: &str,
    name: &str,
) -> Result<Option<TunnelInfo>, ExitError> {
    let client = CloudflareClient::new(api_base.to_string(), token.to_string());
    let tunnels = client
        .list_tunnels(account_id)
        .await
        .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))?;
    Ok(tunnels.into_iter().find(|t| t.name == name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn openrc_cloudflared_script_is_supervised() {
        let script = openrc_cloudflared_script();
        assert!(script.contains("supervisor=supervise-daemon"));
        assert!(script.contains("GOMEMLIMIT=\"${GOMEMLIMIT:-8MiB}\""));
        assert!(
            script.contains(
                "TUNNEL_MANAGEMENT_DIAGNOSTICS=\"${TUNNEL_MANAGEMENT_DIAGNOSTICS:-false}\""
            )
        );
        assert!(script.contains("GOGC=\"${GOGC:-50}\""));
        assert!(script.contains(
            "--protocol ${XP_CLOUDFLARED_PROTOCOL:-http2} --config /etc/cloudflared/config.yml"
        ));
        assert!(script.contains("respawn_delay=2"));
        assert!(script.contains("respawn_max=0"));

        // When supervised by OpenRC, the service should not be backgrounded by the script itself.
        assert!(!script.contains("command_background"));
        assert!(!script.contains("pidfile="));
    }

    #[test]
    fn systemd_cloudflared_unit_defaults_to_http2_with_an_override() {
        let unit = systemd_cloudflared_unit();
        assert!(unit.contains("Environment=XP_CLOUDFLARED_PROTOCOL=http2"));
        assert!(unit.contains("--protocol ${XP_CLOUDFLARED_PROTOCOL} --config"));
    }

    #[test]
    fn load_cloudflare_token_for_deploy_flag_wins() {
        let _lock = crate::ops::util::ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("CLOUDFLARE_API_TOKEN", "envtok") };

        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        fs::create_dir_all(paths.etc_xp_ops_cloudflare_dir()).unwrap();
        fs::write(paths.etc_xp_ops_cloudflare_token(), "filetok").unwrap();

        let (token, src) = load_cloudflare_token_for_deploy(&paths, Some("flagtok"), None).unwrap();
        assert_eq!(token, "flagtok");
        assert_eq!(src, CloudflareTokenSource::Flag);

        unsafe { std::env::remove_var("CLOUDFLARE_API_TOKEN") };
    }

    #[test]
    fn load_cloudflare_token_for_deploy_stdin_wins() {
        let _lock = crate::ops::util::ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("CLOUDFLARE_API_TOKEN", "envtok") };

        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        fs::create_dir_all(paths.etc_xp_ops_cloudflare_dir()).unwrap();
        fs::write(paths.etc_xp_ops_cloudflare_token(), "filetok").unwrap();

        let (token, src) =
            load_cloudflare_token_for_deploy(&paths, None, Some(" stdintok \n")).unwrap();
        assert_eq!(token, "stdintok");
        assert_eq!(src, CloudflareTokenSource::Stdin);

        unsafe { std::env::remove_var("CLOUDFLARE_API_TOKEN") };
    }

    #[test]
    fn load_cloudflare_token_for_deploy_env_wins_over_file() {
        let _lock = crate::ops::util::ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("CLOUDFLARE_API_TOKEN", "envtok") };

        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        fs::create_dir_all(paths.etc_xp_ops_cloudflare_dir()).unwrap();
        fs::write(paths.etc_xp_ops_cloudflare_token(), "filetok").unwrap();

        let (token, src) = load_cloudflare_token_for_deploy(&paths, None, None).unwrap();
        assert_eq!(token, "envtok");
        assert_eq!(src, CloudflareTokenSource::Env);

        unsafe { std::env::remove_var("CLOUDFLARE_API_TOKEN") };
    }

    #[test]
    fn load_cloudflare_token_for_deploy_file_used_when_env_absent() {
        let _lock = crate::ops::util::ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("CLOUDFLARE_API_TOKEN") };

        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        fs::create_dir_all(paths.etc_xp_ops_cloudflare_dir()).unwrap();
        fs::write(paths.etc_xp_ops_cloudflare_token(), " filetok \n").unwrap();

        let (token, src) = load_cloudflare_token_for_deploy(&paths, None, None).unwrap();
        assert_eq!(token, "filetok");
        assert_eq!(src, CloudflareTokenSource::File);
    }

    #[test]
    fn load_cloudflare_token_for_deploy_missing_returns_token_missing() {
        let _lock = crate::ops::util::ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("CLOUDFLARE_API_TOKEN") };

        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());

        let err = load_cloudflare_token_for_deploy(&paths, None, None).unwrap_err();
        assert_eq!(err.code, 3);
        assert_eq!(err.message, "token_missing");
    }

    fn dns_record(content: &str) -> DnsRecordInfo {
        DnsRecordInfo {
            id: "record".to_string(),
            record_type: "CNAME".to_string(),
            name: "xp.example.com".to_string(),
            content: content.to_string(),
            proxied: Some(false),
            ttl: Some(120),
        }
    }

    #[test]
    fn dns_selection_rejects_unmanaged_record_without_migration() {
        let error = cloudflare_provision::select_dns_record(
            &[dns_record("other.cfargotunnel.com")],
            None,
            "xp.example.com",
            "new",
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, 5);
        assert!(error.message.contains("unmanaged or ambiguous"));
    }

    #[test]
    fn dns_selection_allows_verified_previous_tunnel_for_migration() {
        let selected = cloudflare_provision::select_dns_record(
            &[dns_record("old.cfargotunnel.com")],
            Some("record"),
            "xp.example.com",
            "new",
            Some("old"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.content, "old.cfargotunnel.com");
    }

    #[test]
    fn alpine_cloudflared_binary_uses_the_install_path() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());

        assert_eq!(
            cloudflared_binary_path(&paths, Distro::Alpine),
            tmp.path().join("usr/local/bin/cloudflared")
        );
    }
}
