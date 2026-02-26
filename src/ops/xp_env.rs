use crate::admin_token::parse_admin_token_hash;
use crate::ops::cli::ExitError;
use crate::ops::paths::Paths;
use crate::ops::platform::{InitSystem, detect_distro, detect_init_system};
use crate::ops::util::{
    Mode, chmod, ensure_dir, is_test_root, shell_quote_single, shell_unquote_wrapping_quotes,
    write_string_if_changed,
};

#[derive(Default, Debug, Clone, Copy)]
pub struct XpEnvFlags {
    pub has_node_name: bool,
    pub has_access_host: bool,
    pub has_api_base_url: bool,

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
    pub has_cloudflared_restart_mode: bool,
    pub has_cloudflared_restart_cooldown: bool,
    pub has_cloudflared_restart_timeout: bool,
    pub has_cloudflared_systemd_unit: bool,
    pub has_cloudflared_openrc_service: bool,
}

#[derive(Debug, Clone)]
pub struct ParsedXpEnv {
    pub retained_lines: Vec<String>,
    pub admin_token_plain: Option<String>,
    pub admin_token_hash: Option<String>,
    pub node_name: Option<String>,
    pub access_host: Option<String>,
    pub api_base_url: Option<String>,
    pub data_dir: Option<String>,
    pub flags: XpEnvFlags,
}

pub struct XpEnvWriteValues<'a> {
    pub admin_token_hash: &'a str,
    pub node_name: &'a str,
    pub access_host: &'a str,
    pub api_base_url: &'a str,
}

pub fn parse_xp_env(raw: Option<String>) -> ParsedXpEnv {
    let mut retained_lines: Vec<String> = Vec::new();
    let mut admin_token_plain: Option<String> = None;
    let mut admin_token_hash: Option<String> = None;
    let mut node_name: Option<String> = None;
    let mut access_host: Option<String> = None;
    let mut api_base_url: Option<String> = None;
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
        retained_lines.push(line.to_string());
    }

    ParsedXpEnv {
        retained_lines,
        admin_token_plain,
        admin_token_hash,
        node_name,
        access_host,
        api_base_url,
        data_dir,
        flags,
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
        lines.push("XP_XRAY_HEALTH_INTERVAL_SECS=2".to_string());
    }
    if !flags.has_xray_health_fails_before_down {
        lines.push("XP_XRAY_HEALTH_FAILS_BEFORE_DOWN=3".to_string());
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
        lines.push("XP_XRAY_RESTART_TIMEOUT_SECS=5".to_string());
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
    if !flags.has_cloudflared_restart_mode {
        lines.push(format!(
            "XP_CLOUDFLARED_RESTART_MODE={}",
            default_managed_restart_mode(paths)
        ));
    }
    if !flags.has_cloudflared_restart_cooldown {
        lines.push("XP_CLOUDFLARED_RESTART_COOLDOWN_SECS=30".to_string());
    }
    if !flags.has_cloudflared_restart_timeout {
        lines.push("XP_CLOUDFLARED_RESTART_TIMEOUT_SECS=5".to_string());
    }
    if !flags.has_cloudflared_systemd_unit {
        lines.push("XP_CLOUDFLARED_SYSTEMD_UNIT=cloudflared.service".to_string());
    }
    if !flags.has_cloudflared_openrc_service {
        lines.push("XP_CLOUDFLARED_OPENRC_SERVICE=cloudflared".to_string());
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
