use crate::ops::admin_token;
use crate::ops::cloudflare;
use crate::ops::deploy;
use crate::ops::init;
use crate::ops::install;
use crate::ops::paths::Paths;
use crate::ops::preflight;
use crate::ops::status;
use crate::ops::tui;
use crate::ops::upgrade;
use crate::ops::xp;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "xp-ops",
    version = crate::version::VERSION,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Redirect all filesystem writes under this root (test-only).
    #[arg(long, global = true, hide = true, default_value = "/")]
    pub root: PathBuf,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Install(InstallArgs),
    Init(InitArgs),

    Upgrade(UpgradeArgs),

    #[command(subcommand)]
    Xp(XpCommand),

    Deploy(DeployArgs),

    #[command(subcommand)]
    AdminToken(AdminTokenCommand),

    #[command(subcommand)]
    Cloudflare(CloudflareCommand),

    Status(StatusArgs),
    Tui(TuiArgs),
}

#[derive(Args, Debug, Clone)]
pub struct InstallArgs {
    #[arg(long, value_name = "NAME")]
    pub only: Option<InstallOnly>,

    #[arg(long, value_name = "SEMVER|latest", default_value = "latest")]
    pub xray_version: String,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
pub enum InstallOnly {
    Cloudflared,
    Xray,
}

#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    #[arg(long, value_name = "PATH", default_value = "/var/lib/xp")]
    pub xp_work_dir: PathBuf,

    #[arg(long, value_name = "PATH", default_value = "/var/lib/xp/data")]
    pub xp_data_dir: PathBuf,

    #[arg(long, value_name = "PATH", default_value = "/var/lib/xray")]
    pub xray_work_dir: PathBuf,

    #[arg(long, value_enum, value_name = "INIT", default_value = "auto")]
    pub init_system: InitSystemArg,

    #[arg(long)]
    pub enable_services: bool,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
pub enum InitSystemArg {
    Auto,
    Systemd,
    Openrc,
    None,
}

#[derive(Subcommand, Debug)]
pub enum XpCommand {
    Install(XpInstallArgs),
    Bootstrap(XpBootstrapArgs),
    Restart(XpRestartArgs),
    SyncNodeMeta(XpSyncNodeMetaArgs),
    /// Disaster recovery: force this node to become the only Raft voter.
    ///
    /// This is only meant for cases where quorum is permanently lost (e.g. a voter node is wiped).
    RecoverSingleNode(XpRecoverSingleNodeArgs),
}

#[derive(Subcommand, Debug)]
pub enum AdminTokenCommand {
    Show(AdminTokenShowArgs),
    Set(AdminTokenSetArgs),
}

#[derive(Args, Debug, Clone)]
pub struct AdminTokenShowArgs {
    #[arg(long)]
    pub redacted: bool,
}

#[derive(Args, Debug, Clone)]
pub struct AdminTokenSetArgs {
    /// Admin token hash (argon2id PHC string, e.g. `$argon2id$v=19$...`).
    #[arg(long, value_name = "HASH", conflicts_with_all = ["token", "token_stdin"])]
    pub hash: Option<String>,

    /// Admin token plaintext (will be converted to argon2id hash).
    ///
    /// Prefer `--token-stdin` to avoid leaking it via shell history.
    #[arg(long, value_name = "TOKEN", conflicts_with = "token_stdin")]
    pub token: Option<String>,

    /// Read admin token plaintext from stdin (recommended).
    #[arg(long)]
    pub token_stdin: bool,

    /// Keep any existing `XP_ADMIN_TOKEN=...` line in `/etc/xp/xp.env` (not recommended).
    #[arg(long)]
    pub keep_plaintext: bool,

    /// Only print `ok` on stdout (suppress guidance output).
    #[arg(long)]
    pub quiet: bool,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct XpInstallArgs {
    #[arg(long, value_name = "PATH")]
    pub xp_bin: PathBuf,

    #[arg(long)]
    pub enable: bool,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct UpgradeReleaseArgs {
    #[arg(long, value_name = "SEMVER|latest", default_value = "latest")]
    pub version: String,

    #[arg(long)]
    pub prerelease: bool,

    #[arg(long, value_name = "OWNER/REPO")]
    pub repo: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct UpgradeArgs {
    #[command(flatten)]
    pub release: UpgradeReleaseArgs,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct XpBootstrapArgs {
    #[arg(long, value_name = "NAME")]
    pub node_name: String,

    #[arg(long = "access-host", value_name = "HOST")]
    pub access_host: String,

    #[arg(long, value_name = "ORIGIN")]
    pub api_base_url: String,

    #[arg(long, value_name = "PATH", default_value = "/var/lib/xp/data")]
    pub xp_data_dir: PathBuf,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct XpRestartArgs {
    /// Service name to restart (OpenRC: `rc-service <name> restart`, systemd: `<name>.service`).
    #[arg(long, value_name = "NAME", default_value = "xp")]
    pub service_name: String,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct XpSyncNodeMetaArgs {
    /// Local xp API base URL (scheme+host+port), used to talk to the running service.
    ///
    /// Recommended (typical): http://127.0.0.1:62416
    #[arg(long, value_name = "ORIGIN", default_value = "http://127.0.0.1:62416")]
    pub xp_base_url: String,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct XpRecoverSingleNodeArgs {
    /// Skip interactive prompts (required).
    ///
    /// This command performs unsafe changes to the local Raft state.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Do not create a backup copy of the local Raft directory.
    ///
    /// Not recommended; the backup is useful if recovery fails or you need to inspect history.
    #[arg(long)]
    pub no_backup: bool,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DeployArgs {
    #[arg(long, value_name = "PATH")]
    pub xp_bin: Option<PathBuf>,

    #[arg(long, value_name = "NAME")]
    pub node_name: String,

    #[arg(long = "access-host", value_name = "HOST")]
    pub access_host: String,

    #[command(flatten)]
    pub cloudflare_toggle: CloudflareToggle,

    #[arg(long, value_name = "ID")]
    pub account_id: Option<String>,

    #[arg(long, value_name = "ID")]
    pub zone_id: Option<String>,

    #[arg(long, value_name = "FQDN")]
    pub hostname: Option<String>,

    #[arg(long, value_name = "NAME")]
    pub tunnel_name: Option<String>,

    #[arg(long, value_name = "URL")]
    pub origin_url: Option<String>,

    #[arg(long, value_name = "TOKEN", conflicts_with = "join_token_stdin")]
    pub join_token: Option<String>,

    #[arg(long, conflicts_with = "join_token")]
    pub join_token_stdin: bool,

    #[arg(skip)]
    pub join_token_stdin_value: Option<String>,

    #[arg(long, value_name = "TOKEN", conflicts_with = "cloudflare_token_stdin")]
    pub cloudflare_token: Option<String>,

    #[arg(long, conflicts_with = "cloudflare_token")]
    pub cloudflare_token_stdin: bool,

    #[arg(skip)]
    pub cloudflare_token_stdin_value: Option<String>,

    #[arg(long, value_name = "ORIGIN")]
    pub api_base_url: Option<String>,

    #[arg(long, value_name = "SEMVER|latest", default_value = "latest")]
    pub xray_version: String,

    #[command(flatten)]
    pub enable_services_toggle: EnableServicesToggle,

    #[arg(short = 'y', long)]
    pub yes: bool,

    #[arg(long)]
    pub overwrite_existing: bool,

    #[arg(long, alias = "no-prompt")]
    pub non_interactive: bool,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct CloudflareToggle {
    #[arg(long, conflicts_with = "no_cloudflare")]
    pub cloudflare: bool,

    #[arg(long, conflicts_with = "cloudflare")]
    pub no_cloudflare: bool,
}

impl CloudflareToggle {
    pub fn enabled(&self) -> bool {
        !self.no_cloudflare
    }
}

#[derive(Args, Debug, Clone)]
pub struct EnableServicesToggle {
    #[arg(long, conflicts_with = "no_enable_services")]
    pub enable_services: bool,

    #[arg(long, conflicts_with = "enable_services")]
    pub no_enable_services: bool,
}

impl EnableServicesToggle {
    pub fn enabled(&self) -> bool {
        !self.no_enable_services
    }
}

#[derive(Subcommand, Debug)]
pub enum CloudflareCommand {
    Token(CloudflareTokenArgs),
    Provision(CloudflareProvisionArgs),
}

#[derive(Args, Debug)]
pub struct CloudflareTokenArgs {
    #[command(subcommand)]
    pub command: CloudflareTokenCommand,
}

#[derive(Subcommand, Debug)]
pub enum CloudflareTokenCommand {
    Set(CloudflareTokenSetArgs),
}

#[derive(Args, Debug, Clone)]
pub struct CloudflareTokenSetArgs {
    #[arg(long)]
    pub from_stdin: bool,

    #[arg(long, value_name = "NAME")]
    pub from_env: Option<String>,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CloudflareProvisionArgs {
    #[arg(long, value_name = "NAME")]
    pub tunnel_name: Option<String>,

    #[arg(long, value_name = "ID")]
    pub account_id: String,

    #[arg(long, value_name = "ID")]
    pub zone_id: String,

    #[arg(long, value_name = "FQDN")]
    pub hostname: String,

    #[arg(long, value_name = "URL")]
    pub origin_url: String,

    #[arg(long, hide = true, value_name = "ID")]
    pub dns_record_id_override: Option<String>,

    #[arg(long, hide = true, value_name = "ID")]
    pub tunnel_id_override: Option<String>,

    #[arg(long, conflicts_with = "no_enable")]
    pub enable: bool,

    #[arg(long, conflicts_with = "enable")]
    pub no_enable: bool,

    #[arg(long)]
    pub dry_run: bool,
}

impl CloudflareProvisionArgs {
    pub fn enabled(&self) -> bool {
        !self.no_enable
    }
}

#[derive(Args, Debug, Clone)]
pub struct StatusArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TuiArgs {}

#[derive(Debug)]
pub struct ExitError {
    pub code: i32,
    pub message: String,
}

impl ExitError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub async fn run() -> i32 {
    let cli = Cli::parse();
    let paths = Paths::new(cli.root.clone());

    if let Err(e) = preflight::preflight(&paths, &cli.command) {
        eprintln!("{}", e.message);
        return e.code;
    }

    let res: Result<(), ExitError> = match cli.command {
        Some(Command::Install(args)) => install::cmd_install(paths, args).await,
        Some(Command::Init(args)) => init::cmd_init(paths, args).await,
        Some(Command::Upgrade(args)) => upgrade::cmd_upgrade(paths, args).await,
        Some(Command::Xp(cmd)) => match cmd {
            XpCommand::Install(args) => xp::cmd_xp_install(paths, args).await,
            XpCommand::Bootstrap(args) => xp::cmd_xp_bootstrap(paths, args).await,
            XpCommand::Restart(args) => xp::cmd_xp_restart(paths, args).await,
            XpCommand::SyncNodeMeta(args) => xp::cmd_xp_sync_node_meta(paths, args).await,
            XpCommand::RecoverSingleNode(args) => xp::cmd_xp_recover_single_node(paths, args).await,
        },
        Some(Command::Deploy(args)) => deploy::cmd_deploy(paths, args).await,
        Some(Command::AdminToken(cmd)) => match cmd {
            AdminTokenCommand::Show(args) => admin_token::cmd_admin_token_show(paths, args).await,
            AdminTokenCommand::Set(args) => admin_token::cmd_admin_token_set(paths, args).await,
        },
        Some(Command::Cloudflare(cmd)) => match cmd {
            CloudflareCommand::Token(token) => match token.command {
                CloudflareTokenCommand::Set(args) => {
                    cloudflare::cmd_cloudflare_token_set(paths, args).await
                }
            },
            CloudflareCommand::Provision(args) => {
                cloudflare::cmd_cloudflare_provision(paths, args).await
            }
        },
        Some(Command::Status(args)) => status::cmd_status(paths, args).await,
        Some(Command::Tui(_args)) => tui::cmd_tui(paths).await,
        None => tui::cmd_tui(paths).await,
    };

    match res {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e.message);
            e.code
        }
    }
}
