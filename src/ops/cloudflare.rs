use crate::ops::cli::{CloudflareProvisionArgs, CloudflareTokenSetArgs, ExitError};
use crate::ops::install;
use crate::ops::paths::Paths;
use crate::ops::platform::{Distro, InitSystem, detect_distro, detect_init_system};
use crate::ops::util::{
    Mode, chmod, ensure_dir, is_executable, is_test_root, write_string_if_changed,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;

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
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    let distro = detect_distro(&paths).map_err(|e| ExitError::new(2, e))?;
    let init_system = detect_init_system(distro, None);

    ensure_cloudflared_present(&paths, distro, mode).await?;
    ensure_cloudflared_service(&paths, distro, init_system, mode)?;

    if mode == Mode::DryRun {
        eprintln!("would call Cloudflare API (token redacted)");
        eprintln!(
            "would provision: account_id={} zone_id={} hostname={} origin_url={}",
            args.account_id, args.zone_id, args.hostname, args.origin_url
        );
        eprintln!(
            "would write: {}",
            paths.etc_xp_ops_cloudflare_settings().display()
        );
        eprintln!("would write: {}", paths.etc_cloudflared_config().display());
        eprintln!("would write: /etc/cloudflared/<tunnel-id>.json");
        if args.enabled() {
            eprintln!("would enable cloudflared service ({init_system:?})");
        }
        return Ok(());
    }

    let api_base = std::env::var("CLOUDFLARE_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.cloudflare.com".to_string());
    let client = CloudflareClient::new(api_base, token);

    let mut settings = load_settings_or_default(&paths)?;
    settings.enabled = args.enabled();
    settings.install_mode = "external".to_string();
    settings.account_id = args.account_id.clone();
    settings.zone_id = args.zone_id.clone();
    settings.hostname = args.hostname.clone();
    settings.origin_url = args.origin_url.clone();
    if let Some(id) = args.dns_record_id_override.clone() {
        settings.dns_record_id = Some(id);
    }
    if let Some(id) = args.tunnel_id_override.clone() {
        settings.tunnel_id = Some(id);
    }

    let tunnel_name = args
        .tunnel_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "xp".to_string());

    let (tunnel_id, created_new) = if let Some(id) = settings.tunnel_id.clone() {
        (id, false)
    } else {
        let created = client
            .create_tunnel(&args.account_id, &tunnel_name)
            .await
            .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))?;
        let tunnel_id = created.id.clone();
        let cred_path = paths
            .etc_cloudflared_dir()
            .join(format!("{tunnel_id}.json"));
        ensure_dir(&paths.etc_cloudflared_dir())
            .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
        let cred_json = serde_json::to_string_pretty(&created.credentials_file)
            .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
        write_string_if_changed(&cred_path, &(cred_json + "\n"))
            .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
        chmod(&cred_path, 0o600).ok();
        settings.tunnel_id = Some(tunnel_id.clone());
        (tunnel_id, true)
    };

    let cred_path_abs = format!("/etc/cloudflared/{tunnel_id}.json");
    let cred_path = paths
        .etc_cloudflared_dir()
        .join(format!("{tunnel_id}.json"));
    if !created_new && !cred_path.exists() {
        return Err(ExitError::new(
            6,
            format!(
                "filesystem_error: missing credentials file {}",
                cred_path.display()
            ),
        ));
    }
    write_cloudflared_config(&paths, &tunnel_id, &cred_path_abs)?;
    ensure_cloudflared_file_ownership(&paths, &tunnel_id, mode)?;

    client
        .put_tunnel_config(
            &args.account_id,
            &tunnel_id,
            &args.hostname,
            &args.origin_url,
        )
        .await
        .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))?;

    let dns_record_id = if let Some(id) = settings.dns_record_id.clone() {
        client
            .patch_dns_record(&args.zone_id, &id, &args.hostname, &tunnel_id)
            .await
            .map_err(|e| ExitError::new(5, format!("dns_error: {e}")))?;
        id
    } else {
        let id = client
            .create_dns_record(&args.zone_id, &args.hostname, &tunnel_id)
            .await
            .map_err(|e| ExitError::new(5, format!("dns_error: {e}")))?;
        settings.dns_record_id = Some(id.clone());
        id
    };

    settings.tunnel_id = Some(tunnel_id);
    settings.dns_record_id = Some(dns_record_id);
    save_settings(&paths, &settings)?;

    if args.enabled() {
        enable_cloudflared_service(init_system, mode, &paths)?;
    }

    Ok(())
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
    let bin_abs: &Path = match distro {
        Distro::Arch | Distro::Debian => Path::new("/usr/bin/cloudflared"),
        Distro::Alpine => Path::new("/usr/local/bin/cloudflared"),
    };
    let bin = paths.map_abs(bin_abs);
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

fn ensure_cloudflared_service(
    paths: &Paths,
    distro: Distro,
    init_system: InitSystem,
    mode: Mode,
) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would ensure cloudflared service files exist");
        return Ok(());
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
            Distro::Arch | Distro::Debian => Command::new("useradd")
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

    match init_system {
        InitSystem::Systemd => {
            let dir = paths.systemd_unit_dir();
            ensure_dir(&dir).map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            let unit_path = dir.join("cloudflared.service");
            let unit = systemd_cloudflared_unit();
            write_string_if_changed(&unit_path, &unit)
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
        }
        InitSystem::OpenRc => {
            let initd = paths.openrc_initd_dir();
            ensure_dir(&initd).map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            let script_path = initd.join("cloudflared");
            let script = openrc_cloudflared_script();
            write_string_if_changed(&script_path, &script)
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            chmod(&script_path, 0o755).ok();
        }
        InitSystem::None => {}
    }

    Ok(())
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
            let _ = Command::new("rc-update")
                .args(["add", "cloudflared", "default"])
                .status();
            let _ = Command::new("rc-service")
                .args(["cloudflared", "start"])
                .status();
        }
        InitSystem::None => {}
    }
    Ok(())
}

fn write_cloudflared_config(
    paths: &Paths,
    tunnel_id: &str,
    cred_abs: &str,
) -> Result<(), ExitError> {
    let yml = format!("tunnel: {tunnel_id}\ncredentials-file: {cred_abs}\n");
    write_string_if_changed(&paths.etc_cloudflared_config(), &yml)
        .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
    chmod(&paths.etc_cloudflared_config(), 0o640).ok();
    Ok(())
}

fn ensure_cloudflared_file_ownership(
    paths: &Paths,
    tunnel_id: &str,
    mode: Mode,
) -> Result<(), ExitError> {
    if mode == Mode::DryRun || is_test_root(paths.root()) {
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
ExecStart=/usr/bin/cloudflared --no-autoupdate --config /etc/cloudflared/config.yml tunnel run\n\
Restart=always\n\
RestartSec=2s\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n"
        .to_string()
}

fn openrc_cloudflared_script() -> String {
    "#!/sbin/openrc-run\n\nname=\"cloudflared\"\ndescription=\"cloudflared (Cloudflare Tunnel)\"\n\ncommand=\"/usr/local/bin/cloudflared\"\ncommand_args=\"--no-autoupdate --config /etc/cloudflared/config.yml tunnel run\"\ncommand_user=\"cloudflared:cloudflared\"\ncommand_background=\"yes\"\npidfile=\"/run/cloudflared.pid\"\n\ndepend() {\n  need net\n}\n".to_string()
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

fn load_settings_or_default(paths: &Paths) -> Result<Settings, ExitError> {
    let p = paths.etc_xp_ops_cloudflare_settings();
    let Ok(raw) = fs::read_to_string(&p) else {
        return Ok(Settings::default());
    };
    serde_json::from_str(&raw).map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))
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
struct CloudflareClient {
    base: String,
    token: String,
    client: reqwest::Client,
}

impl CloudflareClient {
    fn new(base: String, token: String) -> Self {
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

    async fn put_tunnel_config(
        &self,
        account_id: &str,
        tunnel_id: &str,
        hostname: &str,
        origin_url: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations",
            self.base.trim_end_matches('/')
        );
        let body = serde_json::json!({
          "config": {
            "ingress": [
              { "hostname": hostname, "service": origin_url },
              { "service": "http_status:404" }
            ]
          }
        });
        let resp = self
            .client
            .put(url)
            .bearer_auth(&self.token)
            .json(&body)
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
        hostname: &str,
        tunnel_id: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/client/v4/zones/{zone_id}/dns_records/{dns_record_id}",
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
            .patch(url)
            .bearer_auth(&self.token)
            .json(&body)
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

    async fn list_zones_by_name(&self, name: &str) -> anyhow::Result<Vec<ZoneLookup>> {
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

#[derive(Debug, Clone, Deserialize)]
pub struct DnsRecordInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ZoneInfo {
    pub name: String,
    pub account_id: Option<String>,
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
}
