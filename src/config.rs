use std::{net::SocketAddr, path::PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::admin_token::{AdminTokenHash, hash_admin_token_sha256_legacy, parse_admin_token_hash};

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrayRestartMode {
    None,
    Systemd,
    Openrc,
}

#[derive(Parser, Debug, Clone)]
#[command(
    name = "xp",
    about = "Xray cluster manager",
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
    /// Start the cluster manager HTTP server (default).
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
        long = "xray-health-interval-secs",
        global = true,
        env = "XP_XRAY_HEALTH_INTERVAL_SECS",
        value_name = "SECS",
        default_value_t = 2,
        value_parser = clap::value_parser!(u64).range(1..=30)
    )]
    pub xray_health_interval_secs: u64,

    #[arg(
        long = "xray-health-fails-before-down",
        global = true,
        env = "XP_XRAY_HEALTH_FAILS_BEFORE_DOWN",
        value_name = "N",
        default_value_t = 3,
        value_parser = clap::value_parser!(u64).range(1..=10)
    )]
    pub xray_health_fails_before_down: u64,

    #[arg(
        long = "xray-restart-mode",
        global = true,
        env = "XP_XRAY_RESTART_MODE",
        value_name = "MODE",
        default_value = "none",
        value_enum
    )]
    pub xray_restart_mode: XrayRestartMode,

    #[arg(
        long = "xray-restart-cooldown-secs",
        global = true,
        env = "XP_XRAY_RESTART_COOLDOWN_SECS",
        value_name = "SECS",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..=3600)
    )]
    pub xray_restart_cooldown_secs: u64,

    #[arg(
        long = "xray-restart-timeout-secs",
        global = true,
        env = "XP_XRAY_RESTART_TIMEOUT_SECS",
        value_name = "SECS",
        default_value_t = 5,
        value_parser = clap::value_parser!(u64).range(1..=60)
    )]
    pub xray_restart_timeout_secs: u64,

    #[arg(
        long = "xray-systemd-unit",
        global = true,
        env = "XP_XRAY_SYSTEMD_UNIT",
        value_name = "UNIT",
        default_value = "xray.service"
    )]
    pub xray_systemd_unit: String,

    #[arg(
        long = "xray-openrc-service",
        global = true,
        env = "XP_XRAY_OPENRC_SERVICE",
        value_name = "NAME",
        default_value = "xray"
    )]
    pub xray_openrc_service: String,

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
        env = "XP_ADMIN_TOKEN_HASH",
        value_name = "HASH",
        default_value = ""
    )]
    pub admin_token_hash: String,

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

impl Config {
    /// Normalize admin token configuration:
    /// - Prefer `XP_ADMIN_TOKEN_HASH` when present.
    /// - If only `XP_ADMIN_TOKEN` is present, derive legacy `sha256:<hex>` hash.
    /// - Always clear `admin_token` plaintext after normalization.
    pub fn normalize_admin_token(&mut self) {
        let hash = parse_admin_token_hash(&self.admin_token_hash);
        if let Some(hash) = hash {
            self.admin_token_hash = hash.as_str().to_string();
            self.admin_token.clear();
            return;
        }

        if !self.admin_token.trim().is_empty() {
            // Backward-compat: accept plaintext token but only keep its hash in memory.
            if let Ok(hash) = hash_admin_token_sha256_legacy(self.admin_token.trim()) {
                self.admin_token_hash = hash.as_str().to_string();
            }
        }
        self.admin_token.clear();
    }

    pub fn admin_token_hash(&self) -> Option<AdminTokenHash> {
        parse_admin_token_hash(&self.admin_token_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_when_flags_absent() {
        let cli = Cli::try_parse_from(["xp"]).unwrap();
        assert_eq!(cli.config.xray_health_interval_secs, 2);
        assert_eq!(cli.config.xray_health_fails_before_down, 3);
        assert_eq!(cli.config.xray_restart_mode, XrayRestartMode::None);
        assert_eq!(cli.config.xray_restart_cooldown_secs, 30);
        assert_eq!(cli.config.xray_restart_timeout_secs, 5);
        assert_eq!(cli.config.xray_systemd_unit, "xray.service");
        assert_eq!(cli.config.xray_openrc_service, "xray");
        assert_eq!(cli.config.quota_poll_interval_secs, 10);
        assert!(cli.config.quota_auto_unban);
    }

    #[test]
    fn rejects_invalid_xray_health_interval_secs() {
        let err = Cli::try_parse_from(["xp", "--xray-health-interval-secs", "0"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--xray-health-interval-secs"));
        assert!(msg.contains("1..=30"));
    }

    #[test]
    fn rejects_invalid_xray_health_fails_before_down() {
        let err = Cli::try_parse_from(["xp", "--xray-health-fails-before-down", "0"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--xray-health-fails-before-down"));
        assert!(msg.contains("1..=10"));
    }

    #[test]
    fn rejects_invalid_xray_restart_cooldown_secs() {
        let err = Cli::try_parse_from(["xp", "--xray-restart-cooldown-secs", "0"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--xray-restart-cooldown-secs"));
        assert!(msg.contains("1..=3600"));
    }

    #[test]
    fn rejects_invalid_xray_restart_timeout_secs() {
        let err = Cli::try_parse_from(["xp", "--xray-restart-timeout-secs", "0"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--xray-restart-timeout-secs"));
        assert!(msg.contains("1..=60"));
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
