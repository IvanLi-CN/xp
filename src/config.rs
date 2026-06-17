use std::{net::SocketAddr, path::PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::admin_token::{AdminTokenHash, parse_admin_token_hash};

pub const DEFAULT_VLESS_CANARY_BIND: &str = "127.0.0.1:39043";
pub const DEFAULT_VLESS_CANARY_BIND_PORT: u16 = 39043;
pub const DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE: &str = "/etc/xp/cloudflare_ddns_api_token";

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

    /// Generate a one-time admin login link for the web UI.
    LoginLink,
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
        env = "XP_BIND",
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
        default_value_t = 5,
        value_parser = clap::value_parser!(u64).range(1..=30)
    )]
    pub xray_health_interval_secs: u64,

    #[arg(
        long = "xray-health-fails-before-down",
        global = true,
        env = "XP_XRAY_HEALTH_FAILS_BEFORE_DOWN",
        value_name = "N",
        default_value_t = 4,
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
        default_value_t = 20,
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
        long = "cloudflared-health-interval-secs",
        global = true,
        env = "XP_CLOUDFLARED_HEALTH_INTERVAL_SECS",
        value_name = "SECS",
        default_value_t = 5,
        value_parser = clap::value_parser!(u64).range(1..=60)
    )]
    pub cloudflared_health_interval_secs: u64,

    #[arg(
        long = "cloudflared-health-fails-before-down",
        global = true,
        env = "XP_CLOUDFLARED_HEALTH_FAILS_BEFORE_DOWN",
        value_name = "N",
        default_value_t = 3,
        value_parser = clap::value_parser!(u64).range(1..=10)
    )]
    pub cloudflared_health_fails_before_down: u64,

    #[arg(
        long = "cloudflared-monitor-mode",
        global = true,
        env = "XP_CLOUDFLARED_MONITOR_MODE",
        value_name = "MODE",
        value_enum
    )]
    pub cloudflared_monitor_mode: Option<XrayRestartMode>,

    #[arg(
        long = "cloudflared-restart-mode",
        global = true,
        env = "XP_CLOUDFLARED_RESTART_MODE",
        value_name = "MODE",
        default_value = "none",
        value_enum
    )]
    pub cloudflared_restart_mode: XrayRestartMode,

    #[arg(
        long = "cloudflared-restart-cooldown-secs",
        global = true,
        env = "XP_CLOUDFLARED_RESTART_COOLDOWN_SECS",
        value_name = "SECS",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..=3600)
    )]
    pub cloudflared_restart_cooldown_secs: u64,

    #[arg(
        long = "cloudflared-restart-timeout-secs",
        global = true,
        env = "XP_CLOUDFLARED_RESTART_TIMEOUT_SECS",
        value_name = "SECS",
        default_value_t = 20,
        value_parser = clap::value_parser!(u64).range(1..=60)
    )]
    pub cloudflared_restart_timeout_secs: u64,

    #[arg(
        long = "cloudflared-systemd-unit",
        global = true,
        env = "XP_CLOUDFLARED_SYSTEMD_UNIT",
        value_name = "UNIT",
        default_value = "cloudflared.service"
    )]
    pub cloudflared_systemd_unit: String,

    #[arg(
        long = "cloudflared-openrc-service",
        global = true,
        env = "XP_CLOUDFLARED_OPENRC_SERVICE",
        value_name = "NAME",
        default_value = "cloudflared"
    )]
    pub cloudflared_openrc_service: String,

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
        env = "XP_NODE_NAME",
        value_name = "NAME",
        default_value = "node-1"
    )]
    pub node_name: String,

    #[arg(
        long = "access-host",
        global = true,
        env = "XP_ACCESS_HOST",
        value_name = "HOST",
        default_value = ""
    )]
    pub access_host: String,

    #[arg(
        long,
        global = true,
        env = "XP_API_BASE_URL",
        value_name = "ORIGIN",
        default_value = "https://127.0.0.1:62416"
    )]
    pub api_base_url: String,

    #[arg(
        long = "vless-canary-bind",
        global = true,
        env = "XP_VLESS_CANARY_BIND",
        value_name = "ADDR",
        default_value = DEFAULT_VLESS_CANARY_BIND
    )]
    pub vless_canary_bind: SocketAddr,

    #[arg(
        long = "vless-canary-acme-directory-url",
        global = true,
        env = "XP_VLESS_CANARY_ACME_DIRECTORY_URL",
        value_name = "URL",
        default_value = "https://acme-v02.api.letsencrypt.org/directory"
    )]
    pub vless_canary_acme_directory_url: String,

    #[arg(
        long = "vless-canary-acme-contact-email",
        global = true,
        env = "XP_VLESS_CANARY_ACME_CONTACT_EMAIL",
        value_name = "EMAIL",
        default_value = ""
    )]
    pub vless_canary_acme_contact_email: String,

    #[arg(
        long = "vless-canary-cloudflare-token-file",
        global = true,
        env = "XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE",
        value_name = "PATH",
        default_value = DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE
    )]
    pub vless_canary_cloudflare_token_file: String,

    #[arg(
        long = "vless-canary-cloudflare-zone-id",
        global = true,
        env = "XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID",
        value_name = "ID",
        default_value = ""
    )]
    pub vless_canary_cloudflare_zone_id: String,

    #[arg(
        long = "default-vless-port",
        global = true,
        env = "XP_DEFAULT_VLESS_PORT",
        value_name = "PORT"
    )]
    pub default_vless_port: Option<u16>,

    #[arg(
        long = "default-vless-server-names",
        global = true,
        env = "XP_DEFAULT_VLESS_SERVER_NAMES",
        value_name = "CSV"
    )]
    pub default_vless_server_names: Option<String>,

    #[arg(
        long = "default-vless-fingerprint",
        global = true,
        env = "XP_DEFAULT_VLESS_FINGERPRINT",
        value_name = "NAME"
    )]
    pub default_vless_fingerprint: Option<String>,

    #[arg(
        long = "default-ss-port",
        global = true,
        env = "XP_DEFAULT_SS_PORT",
        value_name = "PORT"
    )]
    pub default_ss_port: Option<u16>,

    #[arg(
        long = "mesh-proxy-url",
        global = true,
        env = "XP_MESH_PROXY_URL",
        value_name = "URL"
    )]
    pub mesh_proxy_url: Option<String>,

    #[arg(
        long = "cloudflare-ddns-enabled",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_ENABLED",
        value_name = "BOOL",
        default_value_t = false,
        action = clap::ArgAction::Set,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub cloudflare_ddns_enabled: bool,

    #[arg(
        long = "cloudflare-ddns-token-file",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_TOKEN_FILE",
        value_name = "PATH",
        default_value = DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE
    )]
    pub cloudflare_ddns_token_file: String,

    #[arg(
        long = "cloudflare-ddns-zone-id",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_ZONE_ID",
        value_name = "ID",
        default_value = ""
    )]
    pub cloudflare_ddns_zone_id: String,

    #[arg(
        long = "cloudflare-ddns-ipv4-url",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_IPV4_URL",
        value_name = "URL",
        default_value = crate::public_ip_probe::DEFAULT_TRACE_URL
    )]
    pub cloudflare_ddns_ipv4_url: String,

    #[arg(
        long = "cloudflare-ddns-ipv6-url",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_IPV6_URL",
        value_name = "URL",
        default_value = crate::public_ip_probe::DEFAULT_TRACE_URL
    )]
    pub cloudflare_ddns_ipv6_url: String,

    #[arg(
        long = "cloudflare-ddns-interval-secs-with-monitor",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_INTERVAL_SECS_WITH_MONITOR",
        value_name = "SECS",
        default_value_t = 300,
        value_parser = clap::value_parser!(u64).range(30..=3600)
    )]
    pub cloudflare_ddns_interval_secs_with_monitor: u64,

    #[arg(
        long = "cloudflare-ddns-interval-secs-no-monitor",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_INTERVAL_SECS_NO_MONITOR",
        value_name = "SECS",
        default_value_t = 60,
        value_parser = clap::value_parser!(u64).range(30..=3600)
    )]
    pub cloudflare_ddns_interval_secs_no_monitor: u64,

    #[arg(
        long = "cloudflare-ddns-fast-interval-secs",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_FAST_INTERVAL_SECS",
        value_name = "SECS",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(10..=300)
    )]
    pub cloudflare_ddns_fast_interval_secs: u64,

    #[arg(
        long = "cloudflare-ddns-fast-window-secs",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_FAST_WINDOW_SECS",
        value_name = "SECS",
        default_value_t = 300,
        value_parser = clap::value_parser!(u64).range(30..=3600)
    )]
    pub cloudflare_ddns_fast_window_secs: u64,

    #[arg(
        long = "cloudflare-ddns-family-missing-grace",
        global = true,
        env = "XP_CLOUDFLARE_DDNS_FAMILY_MISSING_GRACE",
        value_name = "N",
        default_value_t = 3,
        value_parser = clap::value_parser!(u64).range(1..=10)
    )]
    pub cloudflare_ddns_family_missing_grace: u64,

    #[arg(
        long = "endpoint-probe-skip-self-test",
        global = true,
        env = "XP_ENDPOINT_PROBE_SKIP_SELF_TEST",
        value_name = "BOOL",
        default_value_t = false,
        action = clap::ArgAction::Set,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub endpoint_probe_skip_self_test: bool,

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

    #[arg(
        long = "ip-geo-enabled",
        global = true,
        env = "XP_IP_GEO_ENABLED",
        value_name = "BOOL",
        default_value_t = false,
        action = clap::ArgAction::Set,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub ip_geo_enabled: bool,

    #[arg(
        long = "ip-geo-origin",
        global = true,
        env = "XP_IP_GEO_ORIGIN",
        value_name = "ORIGIN",
        default_value = "https://api.country.is"
    )]
    pub ip_geo_origin: String,
}

impl Config {
    pub fn admin_token_hash(&self) -> Option<AdminTokenHash> {
        parse_admin_token_hash(&self.admin_token_hash)
    }

    pub fn effective_cloudflared_monitor_mode(&self) -> XrayRestartMode {
        self.cloudflared_monitor_mode
            .unwrap_or(self.cloudflared_restart_mode)
    }

    pub fn cloudflared_monitoring_enabled(&self) -> bool {
        self.effective_cloudflared_monitor_mode() != XrayRestartMode::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_when_flags_absent() {
        let cli = Cli::try_parse_from(["xp"]).unwrap();
        assert_eq!(cli.config.xray_health_interval_secs, 5);
        assert_eq!(cli.config.xray_health_fails_before_down, 4);
        assert_eq!(cli.config.xray_restart_mode, XrayRestartMode::None);
        assert_eq!(cli.config.xray_restart_cooldown_secs, 30);
        assert_eq!(cli.config.xray_restart_timeout_secs, 20);
        assert_eq!(cli.config.xray_systemd_unit, "xray.service");
        assert_eq!(cli.config.xray_openrc_service, "xray");
        assert_eq!(cli.config.cloudflared_health_interval_secs, 5);
        assert_eq!(cli.config.cloudflared_health_fails_before_down, 3);
        assert_eq!(cli.config.cloudflared_monitor_mode, None);
        assert_eq!(cli.config.cloudflared_restart_mode, XrayRestartMode::None);
        assert_eq!(cli.config.cloudflared_restart_cooldown_secs, 30);
        assert_eq!(cli.config.cloudflared_restart_timeout_secs, 20);
        assert_eq!(cli.config.cloudflared_systemd_unit, "cloudflared.service");
        assert_eq!(cli.config.cloudflared_openrc_service, "cloudflared");
        assert!(!cli.config.cloudflare_ddns_enabled);
        assert_eq!(
            cli.config.cloudflare_ddns_token_file,
            DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE
        );
        assert_eq!(
            cli.config.vless_canary_cloudflare_token_file,
            DEFAULT_CLOUDFLARE_DDNS_TOKEN_FILE
        );
        assert_eq!(cli.config.cloudflare_ddns_zone_id, "");
        assert_eq!(
            cli.config.cloudflare_ddns_ipv4_url,
            crate::public_ip_probe::DEFAULT_TRACE_URL
        );
        assert_eq!(
            cli.config.cloudflare_ddns_ipv6_url,
            crate::public_ip_probe::DEFAULT_TRACE_URL
        );
        assert_eq!(cli.config.cloudflare_ddns_interval_secs_with_monitor, 300);
        assert_eq!(cli.config.cloudflare_ddns_interval_secs_no_monitor, 60);
        assert_eq!(cli.config.cloudflare_ddns_fast_interval_secs, 30);
        assert_eq!(cli.config.cloudflare_ddns_fast_window_secs, 300);
        assert_eq!(cli.config.cloudflare_ddns_family_missing_grace, 3);
        assert_eq!(
            cli.config.vless_canary_bind,
            DEFAULT_VLESS_CANARY_BIND.parse().unwrap()
        );
        assert_eq!(cli.config.mesh_proxy_url, None);
        assert!(!cli.config.endpoint_probe_skip_self_test);
        assert_eq!(cli.config.quota_poll_interval_secs, 10);
        assert!(cli.config.quota_auto_unban);
        assert!(!cli.config.ip_geo_enabled);
        assert_eq!(cli.config.ip_geo_origin, "https://api.country.is");
    }

    #[test]
    fn default_vless_canary_bind_is_loopback_high_port() {
        let cli = Cli::try_parse_from(["xp"]).unwrap();
        assert_eq!(
            cli.config.vless_canary_bind,
            DEFAULT_VLESS_CANARY_BIND.parse().unwrap()
        );
    }

    #[test]
    fn legacy_cloudflared_restart_mode_enables_monitoring() {
        let cli = Cli::try_parse_from(["xp", "--cloudflared-restart-mode", "openrc"]).unwrap();
        assert_eq!(cli.config.cloudflared_monitor_mode, None);
        assert_eq!(cli.config.cloudflared_restart_mode, XrayRestartMode::Openrc);
        assert_eq!(
            cli.config.effective_cloudflared_monitor_mode(),
            XrayRestartMode::Openrc
        );
        assert!(cli.config.cloudflared_monitoring_enabled());
    }

    #[test]
    fn explicit_cloudflared_monitor_none_disables_legacy_fallback() {
        let cli = Cli::try_parse_from([
            "xp",
            "--cloudflared-monitor-mode",
            "none",
            "--cloudflared-restart-mode",
            "openrc",
        ])
        .unwrap();
        assert_eq!(
            cli.config.cloudflared_monitor_mode,
            Some(XrayRestartMode::None)
        );
        assert_eq!(
            cli.config.effective_cloudflared_monitor_mode(),
            XrayRestartMode::None
        );
        assert!(!cli.config.cloudflared_monitoring_enabled());
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
    fn rejects_invalid_cloudflared_health_interval_secs() {
        let err =
            Cli::try_parse_from(["xp", "--cloudflared-health-interval-secs", "0"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--cloudflared-health-interval-secs"));
        assert!(msg.contains("1..=60"));
    }

    #[test]
    fn rejects_invalid_cloudflared_health_fails_before_down() {
        let err =
            Cli::try_parse_from(["xp", "--cloudflared-health-fails-before-down", "0"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--cloudflared-health-fails-before-down"));
        assert!(msg.contains("1..=10"));
    }

    #[test]
    fn rejects_invalid_cloudflared_restart_cooldown_secs() {
        let err =
            Cli::try_parse_from(["xp", "--cloudflared-restart-cooldown-secs", "0"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--cloudflared-restart-cooldown-secs"));
        assert!(msg.contains("1..=3600"));
    }

    #[test]
    fn rejects_invalid_cloudflared_restart_timeout_secs() {
        let err =
            Cli::try_parse_from(["xp", "--cloudflared-restart-timeout-secs", "0"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--cloudflared-restart-timeout-secs"));
        assert!(msg.contains("1..=60"));
    }

    #[test]
    fn rejects_invalid_cloudflare_ddns_interval_secs_with_monitor() {
        let err = Cli::try_parse_from(["xp", "--cloudflare-ddns-interval-secs-with-monitor", "29"])
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--cloudflare-ddns-interval-secs-with-monitor"));
        assert!(msg.contains("30..=3600"));
    }

    #[test]
    fn rejects_invalid_cloudflare_ddns_fast_interval_secs() {
        let err =
            Cli::try_parse_from(["xp", "--cloudflare-ddns-fast-interval-secs", "9"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--cloudflare-ddns-fast-interval-secs"));
        assert!(msg.contains("10..=300"));
    }

    #[test]
    fn rejects_invalid_cloudflare_ddns_family_missing_grace() {
        let err =
            Cli::try_parse_from(["xp", "--cloudflare-ddns-family-missing-grace", "0"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--cloudflare-ddns-family-missing-grace"));
        assert!(msg.contains("1..=10"));
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
