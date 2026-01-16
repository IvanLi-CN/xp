use crate::ops::cloudflare;
use crate::ops::deploy;
use crate::ops::init;
use crate::ops::install;
use crate::ops::paths::Paths;
use crate::ops::status;
use crate::ops::tui;
use crate::ops::xp;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "xp-ops", disable_help_subcommand = true)]
pub struct Cli {
    /// Redirect all filesystem writes under this root (test-only).
    #[arg(long, global = true, hide = true, default_value = "/")]
    pub root: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Install(InstallArgs),
    Init(InitArgs),

    #[command(subcommand)]
    Xp(XpCommand),

    Deploy(DeployArgs),

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
pub struct XpBootstrapArgs {
    #[arg(long, value_name = "NAME")]
    pub node_name: String,

    #[arg(long, value_name = "DOMAIN")]
    pub public_domain: String,

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
    pub xp_bin: PathBuf,

    #[arg(long, value_name = "NAME")]
    pub node_name: String,

    #[arg(long, value_name = "DOMAIN")]
    pub public_domain: String,

    #[command(flatten)]
    pub cloudflare_toggle: CloudflareToggle,

    #[arg(long, value_name = "ID")]
    pub account_id: Option<String>,

    #[arg(long, value_name = "ID")]
    pub zone_id: Option<String>,

    #[arg(long, value_name = "FQDN")]
    pub hostname: Option<String>,

    #[arg(long, value_name = "URL")]
    pub origin_url: Option<String>,

    #[arg(long, value_name = "ORIGIN")]
    pub api_base_url: Option<String>,

    #[arg(long, value_name = "SEMVER|latest", default_value = "latest")]
    pub xray_version: String,

    #[command(flatten)]
    pub enable_services_toggle: EnableServicesToggle,

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
    #[arg(long, value_name = "ID")]
    pub account_id: String,

    #[arg(long, value_name = "ID")]
    pub zone_id: String,

    #[arg(long, value_name = "FQDN")]
    pub hostname: String,

    #[arg(long, value_name = "URL")]
    pub origin_url: String,

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
        Command::Install(args) => install::cmd_install(paths, args).await,
        Command::Init(args) => init::cmd_init(paths, args).await,
        Command::Xp(cmd) => match cmd {
            XpCommand::Install(args) => xp::cmd_xp_install(paths, args).await,
            XpCommand::Bootstrap(args) => xp::cmd_xp_bootstrap(paths, args).await,
        },
        Command::Deploy(args) => deploy::cmd_deploy(paths, args).await,
        Command::Cloudflare(cmd) => match cmd {
            CloudflareCommand::Token(token) => match token.command {
                CloudflareTokenCommand::Set(args) => {
                    cloudflare::cmd_cloudflare_token_set(paths, args).await
                }
            },
            CloudflareCommand::Provision(args) => {
                cloudflare::cmd_cloudflare_provision(paths, args).await
            }
        },
        Command::Status(args) => status::cmd_status(paths, args).await,
        Command::Tui(_args) => tui::cmd_tui(paths).await,
    };

    match res {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e.message);
            e.code
        }
    }
}
