use crate::ops::admin_token;
use crate::ops::cloudflare;
use crate::ops::deploy;
use crate::ops::init;
use crate::ops::install;
use crate::ops::paths::Paths;
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
    SelfUpgrade(SelfUpgradeArgs),

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
    Upgrade(XpUpgradeArgs),
    Bootstrap(XpBootstrapArgs),
}

#[derive(Subcommand, Debug)]
pub enum AdminTokenCommand {
    Show(AdminTokenShowArgs),
}

#[derive(Args, Debug, Clone)]
pub struct AdminTokenShowArgs {
    #[arg(long)]
    pub redacted: bool,
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
pub struct SelfUpgradeArgs {
    #[command(flatten)]
    pub release: UpgradeReleaseArgs,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct XpUpgradeArgs {
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
    let paths = Paths::new(cli.root);

    let res: Result<(), ExitError> = match cli.command {
        Some(Command::Install(args)) => install::cmd_install(paths, args).await,
        Some(Command::Init(args)) => init::cmd_init(paths, args).await,
        Some(Command::SelfUpgrade(args)) => upgrade::cmd_self_upgrade(paths, args).await,
        Some(Command::Xp(cmd)) => match cmd {
            XpCommand::Install(args) => xp::cmd_xp_install(paths, args).await,
            XpCommand::Upgrade(args) => upgrade::cmd_xp_upgrade(paths, args).await,
            XpCommand::Bootstrap(args) => xp::cmd_xp_bootstrap(paths, args).await,
        },
        Some(Command::Deploy(args)) => deploy::cmd_deploy(paths, args).await,
        Some(Command::AdminToken(cmd)) => match cmd {
            AdminTokenCommand::Show(args) => admin_token::cmd_admin_token_show(paths, args).await,
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
