use crate::admin_token::parse_admin_token_hash;
use crate::ops::cli::ExitError;
use crate::ops::paths::Paths;
use crate::ops::platform::{InitSystem, detect_distro, detect_init_system};
use crate::ops::util::{
    Mode, chmod, ensure_dir, is_test_root, shell_quote_single, shell_unquote_wrapping_quotes,
    write_string_if_changed,
};

#[derive(Default, Debug, Clone)]
pub struct XpEnvFlags {
    pub has_node_name: bool,
    pub has_access_host: bool,
    pub has_api_base_url: bool,
    pub has_legacy_relay_probe_enabled: bool,
    pub has_legacy_relay_probe_bind: bool,
    pub has_legacy_relay_probe_origin: bool,
    pub has_legacy_relay_probe_acme_directory_url: bool,
    pub has_legacy_relay_probe_acme_contact_email: bool,
    pub has_legacy_relay_probe_cloudflare_token_file: bool,
    pub has_legacy_relay_probe_cloudflare_zone_id: bool,
    pub has_vless_canary_bind: bool,
    pub has_vless_canary_acme_directory_url: bool,
    pub has_vless_canary_acme_contact_email: bool,
    pub has_vless_canary_cloudflare_token_file: bool,
    pub has_vless_canary_cloudflare_zone_id: bool,
    pub has_default_vless_port: bool,
    pub has_default_vless_server_names: bool,
    pub has_default_vless_fingerprint: bool,
    pub has_default_ss_port: bool,

    pub has_data_dir: bool,
    pub has_xray_addr: bool,
    pub has_xray_health_interval: bool,
    pub has_xray_health_fails_before_down: bool,
    pub has_xray_restart_mode: bool,
    pub has_xray_restart_cooldown: bool,
    pub has_xray_restart_timeout: bool,
    pub has_xray_systemd_unit: bool,
    pub has_xray_openrc_service: bool,

    pub has_cloudflared_health_interval: bool,
    pub has_cloudflared_health_fails_before_down: bool,
    pub has_cloudflared_monitor_mode: bool,
    pub has_cloudflared_restart_mode: bool,
    pub has_cloudflared_restart_cooldown: bool,
    pub has_cloudflared_restart_timeout: bool,
    pub has_cloudflared_systemd_unit: bool,
    pub has_cloudflared_openrc_service: bool,

    pub has_cloudflare_ddns_enabled: bool,
    pub has_cloudflare_ddns_token_file: bool,
    pub has_cloudflare_ddns_zone_id: bool,
    pub has_cloudflare_ddns_ipv4_url: bool,
    pub has_cloudflare_ddns_ipv6_url: bool,
    pub has_cloudflare_ddns_interval_with_monitor: bool,
    pub has_cloudflare_ddns_interval_no_monitor: bool,
    pub has_cloudflare_ddns_fast_interval: bool,
    pub has_cloudflare_ddns_fast_window: bool,
    pub has_cloudflare_ddns_family_missing_grace: bool,
}

#[derive(Debug, Clone)]
pub struct ParsedXpEnv {
    pub retained_lines: Vec<String>,
    pub admin_token_plain: Option<String>,
    pub admin_token_hash: Option<String>,
    pub node_name: Option<String>,
    pub access_host: Option<String>,
    pub api_base_url: Option<String>,
    pub vless_canary_bind: Option<String>,
    pub vless_canary_acme_directory_url: Option<String>,
    pub vless_canary_acme_contact_email: Option<String>,
    pub vless_canary_cloudflare_token_file: Option<String>,
    pub vless_canary_cloudflare_zone_id: Option<String>,
    pub default_vless_port: Option<String>,
    pub default_vless_server_names: Option<String>,
    pub default_vless_fingerprint: Option<String>,
    pub default_ss_port: Option<String>,
    pub data_dir: Option<String>,
    pub flags: XpEnvFlags,
}

pub struct XpEnvWriteValues<'a> {
    pub admin_token_hash: &'a str,
    pub node_name: &'a str,
    pub access_host: &'a str,
    pub api_base_url: &'a str,
    pub vless_canary_bind: &'a str,
    pub vless_canary_acme_directory_url: &'a str,
    pub vless_canary_acme_contact_email: &'a str,
    pub vless_canary_cloudflare_token_file: &'a str,
    pub vless_canary_cloudflare_zone_id: &'a str,
    pub default_vless_port: Option<&'a str>,
    pub default_vless_server_names: Option<&'a str>,
    pub default_vless_fingerprint: Option<&'a str>,
    pub default_ss_port: Option<&'a str>,
    pub cloudflare_ddns_enabled: bool,
    pub cloudflare_ddns_token_file: &'a str,
    pub cloudflare_ddns_zone_id: &'a str,
}

pub fn parse_xp_env(raw: Option<String>) -> ParsedXpEnv {
    let mut retained_lines: Vec<String> = Vec::new();
    let mut admin_token_plain: Option<String> = None;
    let mut admin_token_hash: Option<String> = None;
    let mut node_name: Option<String> = None;
    let mut access_host: Option<String> = None;
    let mut api_base_url: Option<String> = None;
    let mut vless_canary_bind: Option<String> = None;
    let mut vless_canary_acme_directory_url: Option<String> = None;
    let mut vless_canary_acme_contact_email: Option<String> = None;
    let mut vless_canary_cloudflare_token_file: Option<String> = None;
    let mut vless_canary_cloudflare_zone_id: Option<String> = None;
    let mut default_vless_port: Option<String> = None;
    let mut default_vless_server_names: Option<String> = None;
    let mut default_vless_fingerprint: Option<String> = None;
    let mut default_ss_port: Option<String> = None;
    let mut data_dir: Option<String> = None;
    let mut flags = XpEnvFlags::default();

    let Some(s) = raw else {
        return ParsedXpEnv {
            retained_lines,
            admin_token_plain,
            admin_token_hash,
            node_name,
            access_host,
            api_base_url,
            vless_canary_bind,
            vless_canary_acme_directory_url,
            vless_canary_acme_contact_email,
            vless_canary_cloudflare_token_file,
            vless_canary_cloudflare_zone_id,
            default_vless_port,
            default_vless_server_names,
            default_vless_fingerprint,
            default_ss_port,
            data_dir,
            flags,
        };
    };

    for line in s.lines() {
        let line = line.trim();
        if line.starts_with("XP_ADMIN_TOKEN_HASH=") {
            if line.len() > "XP_ADMIN_TOKEN_HASH=".len() {
                admin_token_hash = Some(
                    shell_unquote_wrapping_quotes(
                        line.trim_start_matches("XP_ADMIN_TOKEN_HASH=").trim(),
                    )
                    .to_string(),
                );
            }
            continue;
        }
        if line.starts_with("XP_ADMIN_TOKEN=") {
            if line.len() > "XP_ADMIN_TOKEN=".len() {
                admin_token_plain = Some(
                    shell_unquote_wrapping_quotes(
                        line.trim_start_matches("XP_ADMIN_TOKEN=").trim(),
                    )
                    .to_string(),
                );
            }
            continue;
        }

        if line.starts_with("XP_NODE_NAME=") {
            flags.has_node_name = true;
            if line.len() > "XP_NODE_NAME=".len() {
                node_name = Some(
                    shell_unquote_wrapping_quotes(line.trim_start_matches("XP_NODE_NAME="))
                        .to_string(),
                );
            }
            continue;
        }
        if line.starts_with("XP_ACCESS_HOST=") {
            flags.has_access_host = true;
            if line.len() > "XP_ACCESS_HOST=".len() {
                access_host = Some(
                    shell_unquote_wrapping_quotes(line.trim_start_matches("XP_ACCESS_HOST="))
                        .to_string(),
                );
            }
            continue;
        }
        if line.starts_with("XP_API_BASE_URL=") {
            flags.has_api_base_url = true;
            if line.len() > "XP_API_BASE_URL=".len() {
                api_base_url = Some(
                    shell_unquote_wrapping_quotes(line.trim_start_matches("XP_API_BASE_URL="))
                        .to_string(),
                );
            }
            continue;
        }
        if line.starts_with("XP_RELAY_PROBE_ENABLED=") {
            flags.has_legacy_relay_probe_enabled = true;
            continue;
        }
        if line.starts_with("XP_RELAY_PROBE_BIND=") {
            flags.has_legacy_relay_probe_bind = true;
            continue;
        }
        if line.starts_with("XP_RELAY_PROBE_ORIGIN=") {
            flags.has_legacy_relay_probe_origin = true;
            continue;
        }
        if line.starts_with("XP_RELAY_PROBE_ACME_DIRECTORY_URL=") {
            flags.has_legacy_relay_probe_acme_directory_url = true;
            continue;
        }
        if line.starts_with("XP_RELAY_PROBE_ACME_CONTACT_EMAIL=") {
            flags.has_legacy_relay_probe_acme_contact_email = true;
            continue;
        }
        if line.starts_with("XP_RELAY_PROBE_CLOUDFLARE_TOKEN_FILE=") {
            flags.has_legacy_relay_probe_cloudflare_token_file = true;
            continue;
        }
        if line.starts_with("XP_RELAY_PROBE_CLOUDFLARE_ZONE_ID=") {
            flags.has_legacy_relay_probe_cloudflare_zone_id = true;
            continue;
        }
        if line.starts_with("XP_VLESS_CANARY_BIND=") {
            flags.has_vless_canary_bind = true;
            if line.len() > "XP_VLESS_CANARY_BIND=".len() {
                vless_canary_bind = Some(
                    shell_unquote_wrapping_quotes(line.trim_start_matches("XP_VLESS_CANARY_BIND="))
                        .to_string(),
                );
            }
            continue;
        }
        if line.starts_with("XP_VLESS_CANARY_ACME_DIRECTORY_URL=") {
            flags.has_vless_canary_acme_directory_url = true;
            if line.len() > "XP_VLESS_CANARY_ACME_DIRECTORY_URL=".len() {
                vless_canary_acme_directory_url = Some(
                    shell_unquote_wrapping_quotes(
                        line.trim_start_matches("XP_VLESS_CANARY_ACME_DIRECTORY_URL="),
                    )
                    .to_string(),
                );
            }
            continue;
        }
        if line.starts_with("XP_VLESS_CANARY_ACME_CONTACT_EMAIL=") {
            flags.has_vless_canary_acme_contact_email = true;
            if line.len() > "XP_VLESS_CANARY_ACME_CONTACT_EMAIL=".len() {
                vless_canary_acme_contact_email = Some(
                    shell_unquote_wrapping_quotes(
                        line.trim_start_matches("XP_VLESS_CANARY_ACME_CONTACT_EMAIL="),
                    )
                    .to_string(),
                );
            }
            continue;
        }
        if line.starts_with("XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE=") {
            flags.has_vless_canary_cloudflare_token_file = true;
            if line.len() > "XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE=".len() {
                vless_canary_cloudflare_token_file = Some(
                    shell_unquote_wrapping_quotes(
                        line.trim_start_matches("XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE="),
                    )
                    .to_string(),
                );
            }
            continue;
        }
        if line.starts_with("XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID=") {
            flags.has_vless_canary_cloudflare_zone_id = true;
            if line.len() > "XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID=".len() {
                vless_canary_cloudflare_zone_id = Some(
                    shell_unquote_wrapping_quotes(
                        line.trim_start_matches("XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID="),
                    )
                    .to_string(),
                );
            }
            continue;
        }
        if line.starts_with("XP_DEFAULT_VLESS_PORT=") {
            flags.has_default_vless_port = true;
            if line.len() > "XP_DEFAULT_VLESS_PORT=".len() {
                default_vless_port = Some(
                    shell_unquote_wrapping_quotes(line.trim_start_matches("XP_DEFAULT_VLESS_PORT="))
                        .to_string(),
                );
            }
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_DEFAULT_VLESS_SERVER_NAMES=") {
            flags.has_default_vless_server_names = true;
            if line.len() > "XP_DEFAULT_VLESS_SERVER_NAMES=".len() {
                default_vless_server_names = Some(
                    shell_unquote_wrapping_quotes(
                        line.trim_start_matches("XP_DEFAULT_VLESS_SERVER_NAMES="),
                    )
                    .to_string(),
                );
            }
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_DEFAULT_VLESS_FINGERPRINT=") {
            flags.has_default_vless_fingerprint = true;
            if line.len() > "XP_DEFAULT_VLESS_FINGERPRINT=".len() {
                default_vless_fingerprint = Some(
                    shell_unquote_wrapping_quotes(
                        line.trim_start_matches("XP_DEFAULT_VLESS_FINGERPRINT="),
                    )
                    .to_string(),
                );
            }
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_DEFAULT_SS_PORT=") {
            flags.has_default_ss_port = true;
            if line.len() > "XP_DEFAULT_SS_PORT=".len() {
                default_ss_port = Some(
                    shell_unquote_wrapping_quotes(line.trim_start_matches("XP_DEFAULT_SS_PORT="))
                        .to_string(),
                );
            }
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_ENABLED=") {
            flags.has_cloudflare_ddns_enabled = true;
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_TOKEN_FILE=") {
            flags.has_cloudflare_ddns_token_file = true;
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_ZONE_ID=") {
            flags.has_cloudflare_ddns_zone_id = true;
            continue;
        }

        if line.starts_with("XP_DATA_DIR=") {
            flags.has_data_dir = true;
            if line.len() > "XP_DATA_DIR=".len() {
                data_dir = Some(
                    shell_unquote_wrapping_quotes(line.trim_start_matches("XP_DATA_DIR="))
                        .to_string(),
                );
            }
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_XRAY_API_ADDR=") {
            flags.has_xray_addr = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_XRAY_HEALTH_INTERVAL_SECS=") {
            flags.has_xray_health_interval = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_XRAY_HEALTH_FAILS_BEFORE_DOWN=") {
            flags.has_xray_health_fails_before_down = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_XRAY_RESTART_MODE=") {
            flags.has_xray_restart_mode = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_XRAY_RESTART_COOLDOWN_SECS=") {
            flags.has_xray_restart_cooldown = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_XRAY_RESTART_TIMEOUT_SECS=") {
            flags.has_xray_restart_timeout = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_XRAY_SYSTEMD_UNIT=") {
            flags.has_xray_systemd_unit = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_XRAY_OPENRC_SERVICE=") {
            flags.has_xray_openrc_service = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARED_HEALTH_INTERVAL_SECS=") {
            flags.has_cloudflared_health_interval = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARED_HEALTH_FAILS_BEFORE_DOWN=") {
            flags.has_cloudflared_health_fails_before_down = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARED_MONITOR_MODE=") {
            flags.has_cloudflared_monitor_mode = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARED_RESTART_MODE=") {
            flags.has_cloudflared_restart_mode = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARED_RESTART_COOLDOWN_SECS=") {
            flags.has_cloudflared_restart_cooldown = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARED_RESTART_TIMEOUT_SECS=") {
            flags.has_cloudflared_restart_timeout = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARED_SYSTEMD_UNIT=") {
            flags.has_cloudflared_systemd_unit = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARED_OPENRC_SERVICE=") {
            flags.has_cloudflared_openrc_service = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_IPV4_URL=") {
            flags.has_cloudflare_ddns_ipv4_url = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_IPV6_URL=") {
            flags.has_cloudflare_ddns_ipv6_url = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_INTERVAL_SECS_WITH_MONITOR=") {
            flags.has_cloudflare_ddns_interval_with_monitor = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_INTERVAL_SECS_NO_MONITOR=") {
            flags.has_cloudflare_ddns_interval_no_monitor = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_FAST_INTERVAL_SECS=") {
            flags.has_cloudflare_ddns_fast_interval = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_FAST_WINDOW_SECS=") {
            flags.has_cloudflare_ddns_fast_window = true;
            retained_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("XP_CLOUDFLARE_DDNS_FAMILY_MISSING_GRACE=") {
            flags.has_cloudflare_ddns_family_missing_grace = true;
            retained_lines.push(line.to_string());
            continue;
        }
        retained_lines.push(line.to_string());
    }

    ParsedXpEnv {
        retained_lines,
        admin_token_plain,
        admin_token_hash,
        node_name,
        access_host,
        api_base_url,
        vless_canary_bind,
        vless_canary_acme_directory_url,
        vless_canary_acme_contact_email,
        vless_canary_cloudflare_token_file,
        vless_canary_cloudflare_zone_id,
        default_vless_port,
        default_vless_server_names,
        default_vless_fingerprint,
        default_ss_port,
        data_dir,
        flags,
    }
}

impl ParsedXpEnv {
    pub fn has_legacy_relay_probe_vars(&self) -> bool {
        self.flags.has_legacy_relay_probe_enabled
            || self.flags.has_legacy_relay_probe_bind
            || self.flags.has_legacy_relay_probe_origin
            || self.flags.has_legacy_relay_probe_acme_directory_url
            || self.flags.has_legacy_relay_probe_acme_contact_email
            || self.flags.has_legacy_relay_probe_cloudflare_token_file
            || self.flags.has_legacy_relay_probe_cloudflare_zone_id
    }
}

fn default_managed_restart_mode(paths: &Paths) -> &'static str {
    if is_test_root(paths.root()) {
        return "none";
    }
    let distro = detect_distro(paths).ok();
    let init_system = distro.map(|d| detect_init_system(d, None));
    match init_system {
        Some(InitSystem::Systemd) => "systemd",
        Some(InitSystem::OpenRc) => "openrc",
        _ => "none",
    }
}

pub fn write_xp_env(
    paths: &Paths,
    mode: Mode,
    retained_lines: Vec<String>,
    flags: XpEnvFlags,
    values: XpEnvWriteValues<'_>,
) -> Result<(), ExitError> {
    let p = paths.etc_xp_env();
    if mode == Mode::DryRun {
        eprintln!("would ensure: {}", p.display());
        return Ok(());
    }

    ensure_dir(&paths.etc_xp_dir())
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;

    if parse_admin_token_hash(values.admin_token_hash).is_none() {
        return Err(ExitError::new(
            2,
            "invalid_input: XP_ADMIN_TOKEN_HASH is invalid",
        ));
    }

    let mut lines = retained_lines;

    let node_name = shell_quote_single(values.node_name).map_err(|e| {
        ExitError::new(
            2,
            format!("invalid_input: XP_NODE_NAME cannot be written safely: {e}"),
        )
    })?;
    let access_host = shell_quote_single(values.access_host).map_err(|e| {
        ExitError::new(
            2,
            format!("invalid_input: XP_ACCESS_HOST cannot be written safely: {e}"),
        )
    })?;
    let api_base_url = shell_quote_single(values.api_base_url).map_err(|e| {
        ExitError::new(
            2,
            format!("invalid_input: XP_API_BASE_URL cannot be written safely: {e}"),
        )
    })?;
    lines.push(format!("XP_NODE_NAME={node_name}"));
    lines.push(format!("XP_ACCESS_HOST={access_host}"));
    lines.push(format!("XP_API_BASE_URL={api_base_url}"));
    let vless_canary_bind = shell_quote_single(values.vless_canary_bind).map_err(|e| {
        ExitError::new(
            2,
            format!("invalid_input: XP_VLESS_CANARY_BIND cannot be written safely: {e}"),
        )
    })?;
    lines.push(format!("XP_VLESS_CANARY_BIND={vless_canary_bind}"));
    let vless_canary_acme_directory_url =
        shell_quote_single(values.vless_canary_acme_directory_url).map_err(|e| {
            ExitError::new(
                2,
                format!(
                    "invalid_input: XP_VLESS_CANARY_ACME_DIRECTORY_URL cannot be written safely: {e}"
                ),
            )
        })?;
    lines.push(format!(
        "XP_VLESS_CANARY_ACME_DIRECTORY_URL={vless_canary_acme_directory_url}"
    ));
    let vless_canary_acme_contact_email =
        shell_quote_single(values.vless_canary_acme_contact_email).map_err(|e| {
            ExitError::new(
                2,
                format!(
                    "invalid_input: XP_VLESS_CANARY_ACME_CONTACT_EMAIL cannot be written safely: {e}"
                ),
            )
        })?;
    lines.push(format!(
        "XP_VLESS_CANARY_ACME_CONTACT_EMAIL={vless_canary_acme_contact_email}"
    ));
    let vless_canary_cloudflare_token_file =
        shell_quote_single(values.vless_canary_cloudflare_token_file).map_err(|e| {
            ExitError::new(
                2,
                format!(
                    "invalid_input: XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE cannot be written safely: {e}"
                ),
            )
        })?;
    lines.push(format!(
        "XP_VLESS_CANARY_CLOUDFLARE_TOKEN_FILE={vless_canary_cloudflare_token_file}"
    ));
    let vless_canary_cloudflare_zone_id =
        shell_quote_single(values.vless_canary_cloudflare_zone_id).map_err(|e| {
            ExitError::new(
                2,
                format!(
                    "invalid_input: XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID cannot be written safely: {e}"
                ),
            )
        })?;
    lines.push(format!(
        "XP_VLESS_CANARY_CLOUDFLARE_ZONE_ID={vless_canary_cloudflare_zone_id}"
    ));
    if let Some(default_vless_port) = values.default_vless_port {
        let default_vless_port = shell_quote_single(default_vless_port).map_err(|e| {
            ExitError::new(
                2,
                format!("invalid_input: XP_DEFAULT_VLESS_PORT cannot be written safely: {e}"),
            )
        })?;
        lines.push(format!("XP_DEFAULT_VLESS_PORT={default_vless_port}"));
    } else if !flags.has_default_vless_port {
        // Leave unmanaged when not explicitly configured.
    }
    if let Some(default_vless_server_names) = values.default_vless_server_names {
        let default_vless_server_names =
            shell_quote_single(default_vless_server_names).map_err(|e| {
                ExitError::new(
                    2,
                    format!(
                        "invalid_input: XP_DEFAULT_VLESS_SERVER_NAMES cannot be written safely: {e}"
                    ),
                )
            })?;
        lines.push(format!(
            "XP_DEFAULT_VLESS_SERVER_NAMES={default_vless_server_names}"
        ));
    }
    if let Some(default_vless_fingerprint) = values.default_vless_fingerprint {
        let default_vless_fingerprint =
            shell_quote_single(default_vless_fingerprint).map_err(|e| {
                ExitError::new(
                    2,
                    format!(
                        "invalid_input: XP_DEFAULT_VLESS_FINGERPRINT cannot be written safely: {e}"
                    ),
                )
            })?;
        lines.push(format!(
            "XP_DEFAULT_VLESS_FINGERPRINT={default_vless_fingerprint}"
        ));
    }
    if let Some(default_ss_port) = values.default_ss_port {
        let default_ss_port = shell_quote_single(default_ss_port).map_err(|e| {
            ExitError::new(
                2,
                format!("invalid_input: XP_DEFAULT_SS_PORT cannot be written safely: {e}"),
            )
        })?;
        lines.push(format!("XP_DEFAULT_SS_PORT={default_ss_port}"));
    }
    lines.push(format!(
        "XP_CLOUDFLARE_DDNS_ENABLED={}",
        if values.cloudflare_ddns_enabled {
            "true"
        } else {
            "false"
        }
    ));
    let ddns_token_file = shell_quote_single(values.cloudflare_ddns_token_file).map_err(|e| {
        ExitError::new(
            2,
            format!("invalid_input: XP_CLOUDFLARE_DDNS_TOKEN_FILE cannot be written safely: {e}"),
        )
    })?;
    lines.push(format!("XP_CLOUDFLARE_DDNS_TOKEN_FILE={ddns_token_file}"));
    let ddns_zone_id = shell_quote_single(values.cloudflare_ddns_zone_id).map_err(|e| {
        ExitError::new(
            2,
            format!("invalid_input: XP_CLOUDFLARE_DDNS_ZONE_ID cannot be written safely: {e}"),
        )
    })?;
    lines.push(format!("XP_CLOUDFLARE_DDNS_ZONE_ID={ddns_zone_id}"));

    let quoted = shell_quote_single(values.admin_token_hash).map_err(|e| {
        ExitError::new(
            2,
            format!("invalid_input: XP_ADMIN_TOKEN_HASH cannot be written safely: {e}"),
        )
    })?;
    lines.push(format!("XP_ADMIN_TOKEN_HASH={quoted}"));

    if !flags.has_data_dir {
        lines.push("XP_DATA_DIR=/var/lib/xp/data".to_string());
    }
    if !flags.has_xray_addr {
        lines.push("XP_XRAY_API_ADDR=127.0.0.1:10085".to_string());
    }
    if !flags.has_xray_health_interval {
        lines.push("XP_XRAY_HEALTH_INTERVAL_SECS=5".to_string());
    }
    if !flags.has_xray_health_fails_before_down {
        lines.push("XP_XRAY_HEALTH_FAILS_BEFORE_DOWN=4".to_string());
    }
    if !flags.has_xray_restart_mode {
        lines.push(format!(
            "XP_XRAY_RESTART_MODE={}",
            default_managed_restart_mode(paths)
        ));
    }
    if !flags.has_xray_restart_cooldown {
        lines.push("XP_XRAY_RESTART_COOLDOWN_SECS=30".to_string());
    }
    if !flags.has_xray_restart_timeout {
        lines.push("XP_XRAY_RESTART_TIMEOUT_SECS=20".to_string());
    }
    if !flags.has_xray_systemd_unit {
        lines.push("XP_XRAY_SYSTEMD_UNIT=xray.service".to_string());
    }
    if !flags.has_xray_openrc_service {
        lines.push("XP_XRAY_OPENRC_SERVICE=xray".to_string());
    }
    if !flags.has_cloudflared_health_interval {
        lines.push("XP_CLOUDFLARED_HEALTH_INTERVAL_SECS=5".to_string());
    }
    if !flags.has_cloudflared_health_fails_before_down {
        lines.push("XP_CLOUDFLARED_HEALTH_FAILS_BEFORE_DOWN=3".to_string());
    }
    if !flags.has_cloudflared_monitor_mode && !flags.has_cloudflared_restart_mode {
        lines.push(format!(
            "XP_CLOUDFLARED_MONITOR_MODE={}",
            default_managed_restart_mode(paths)
        ));
    }
    if !flags.has_cloudflared_restart_mode {
        lines.push("XP_CLOUDFLARED_RESTART_MODE=none".to_string());
    }
    if !flags.has_cloudflared_restart_cooldown {
        lines.push("XP_CLOUDFLARED_RESTART_COOLDOWN_SECS=30".to_string());
    }
    if !flags.has_cloudflared_restart_timeout {
        lines.push("XP_CLOUDFLARED_RESTART_TIMEOUT_SECS=20".to_string());
    }
    if !flags.has_cloudflared_systemd_unit {
        lines.push("XP_CLOUDFLARED_SYSTEMD_UNIT=cloudflared.service".to_string());
    }
    if !flags.has_cloudflared_openrc_service {
        lines.push("XP_CLOUDFLARED_OPENRC_SERVICE=cloudflared".to_string());
    }
    if !flags.has_cloudflare_ddns_ipv4_url {
        lines.push(format!(
            "XP_CLOUDFLARE_DDNS_IPV4_URL={}",
            crate::ddns::DEFAULT_TRACE_URL
        ));
    }
    if !flags.has_cloudflare_ddns_ipv6_url {
        lines.push(format!(
            "XP_CLOUDFLARE_DDNS_IPV6_URL={}",
            crate::ddns::DEFAULT_TRACE_URL
        ));
    }
    if !flags.has_cloudflare_ddns_interval_with_monitor {
        lines.push("XP_CLOUDFLARE_DDNS_INTERVAL_SECS_WITH_MONITOR=300".to_string());
    }
    if !flags.has_cloudflare_ddns_interval_no_monitor {
        lines.push("XP_CLOUDFLARE_DDNS_INTERVAL_SECS_NO_MONITOR=60".to_string());
    }
    if !flags.has_cloudflare_ddns_fast_interval {
        lines.push("XP_CLOUDFLARE_DDNS_FAST_INTERVAL_SECS=30".to_string());
    }
    if !flags.has_cloudflare_ddns_fast_window {
        lines.push("XP_CLOUDFLARE_DDNS_FAST_WINDOW_SECS=300".to_string());
    }
    if !flags.has_cloudflare_ddns_family_missing_grace {
        lines.push("XP_CLOUDFLARE_DDNS_FAMILY_MISSING_GRACE=3".to_string());
    }

    let content = format!("{}\n", lines.join("\n"));
    write_string_if_changed(&p, &content)
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    chmod(&p, 0o640).ok();
    if !is_test_root(paths.root()) {
        let _ = std::process::Command::new("chown")
            .args(["root:xp", p.to_string_lossy().as_ref()])
            .status();
    }
    Ok(())
}
