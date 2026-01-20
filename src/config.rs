use std::{net::SocketAddr, path::PathBuf};

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "xp",
    about = "Xray control plane",
    version = crate::version::VERSION,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub config: Config,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Start the control-plane HTTP server (default).
    Run,

    /// Initialize cluster identity and bootstrap state under --data-dir.
    Init,

    /// Join an existing cluster using a join token.
    Join(JoinArgs),
}

#[derive(Args, Debug, Clone)]
pub struct JoinArgs {
    #[arg(long, value_name = "TOKEN")]
    pub token: String,
}

#[derive(Args, Debug, Clone)]
pub struct Config {
    #[arg(
        long,
        global = true,
        value_name = "ADDR",
        default_value = "127.0.0.1:62416"
    )]
    pub bind: SocketAddr,

    #[arg(
        long,
        global = true,
        env = "XP_XRAY_API_ADDR",
        value_name = "ADDR",
        default_value = "127.0.0.1:10085"
    )]
    pub xray_api_addr: SocketAddr,

    #[arg(
        long,
        global = true,
        env = "XP_DATA_DIR",
        value_name = "PATH",
        default_value = "./data"
    )]
    pub data_dir: PathBuf,

    #[arg(
        long,
        global = true,
        env = "XP_ADMIN_TOKEN",
        value_name = "TOKEN",
        default_value = ""
    )]
    pub admin_token: String,

    #[arg(long, global = true, value_name = "NAME", default_value = "node-1")]
    pub node_name: String,

    #[arg(
        long = "access-host",
        global = true,
        value_name = "HOST",
        default_value = ""
    )]
    pub access_host: String,

    #[arg(
        long,
        global = true,
        value_name = "ORIGIN",
        default_value = "https://127.0.0.1:62416"
    )]
    pub api_base_url: String,

    #[arg(
        long = "quota-poll-interval-secs",
        global = true,
        env = "XP_QUOTA_POLL_INTERVAL_SECS",
        value_name = "SECS",
        default_value_t = 10,
        value_parser = clap::value_parser!(u64).range(5..=30)
    )]
    pub quota_poll_interval_secs: u64,

    #[arg(
        long = "quota-auto-unban",
        global = true,
        env = "XP_QUOTA_AUTO_UNBAN",
        value_name = "BOOL",
        default_value_t = true,
        action = clap::ArgAction::Set,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub quota_auto_unban: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_when_flags_absent() {
        let cli = Cli::try_parse_from(["xp"]).unwrap();
        assert_eq!(cli.config.quota_poll_interval_secs, 10);
        assert!(cli.config.quota_auto_unban);
    }

    #[test]
    fn rejects_invalid_quota_poll_interval_secs() {
        let err = Cli::try_parse_from(["xp", "--quota-poll-interval-secs", "4"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--quota-poll-interval-secs"));
        assert!(msg.contains("5..=30"));
    }

    #[test]
    fn parses_quota_auto_unban_as_bool_value() {
        let cli = Cli::try_parse_from(["xp", "--quota-auto-unban", "false"]).unwrap();
        assert!(!cli.config.quota_auto_unban);
    }
}
