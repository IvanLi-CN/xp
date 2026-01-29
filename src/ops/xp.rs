use crate::ops::cli::{ExitError, XpBootstrapArgs, XpInstallArgs, XpRestartArgs};
use crate::ops::paths::Paths;
use crate::ops::util::{Mode, chmod, ensure_dir, is_test_root, write_bytes_if_changed};
use std::fs;
use std::path::Path;
use std::process::Command;

pub async fn cmd_xp_install(paths: Paths, args: XpInstallArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    if mode == Mode::DryRun {
        eprintln!("would copy: {} -> /usr/local/bin/xp", args.xp_bin.display());
        if args.enable {
            eprintln!("would enable and start xp service (init-system auto)");
        }
        return Ok(());
    }

    let src = args.xp_bin;
    if !src.exists() {
        return Err(ExitError::new(2, "invalid_args: --xp-bin does not exist"));
    }

    let dest = paths.usr_local_bin_xp();
    if let Some(parent) = dest.parent() {
        ensure_dir(parent).map_err(|e| ExitError::new(3, format!("filesystem_error: {e}")))?;
    }

    let bytes = fs::read(&src).map_err(|e| ExitError::new(3, format!("filesystem_error: {e}")))?;
    write_bytes_if_changed(&dest, &bytes)
        .map_err(|e| ExitError::new(3, format!("filesystem_error: {e}")))?;
    chmod(&dest, 0o755).ok();

    if !is_test_root(paths.root()) {
        let status = Command::new("/usr/local/bin/xp")
            .arg("--help")
            .status()
            .map_err(|e| ExitError::new(3, format!("filesystem_error: xp verify: {e}")))?;
        if !status.success() {
            return Err(ExitError::new(3, "filesystem_error: xp verify failed"));
        }
    }

    if args.enable && !is_test_root(paths.root()) {
        // Defer to init-system auto behavior: try systemd first, then OpenRC.
        if Command::new("systemctl")
            .args(["enable", "--now", "xp.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Ok(());
        }
        let _ = Command::new("rc-update")
            .args(["add", "xp", "default"])
            .status();
        let _ = Command::new("rc-service").args(["xp", "start"]).status();
    }

    Ok(())
}

pub async fn cmd_xp_bootstrap(paths: Paths, args: XpBootstrapArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    validate_https_origin(&args.api_base_url)?;

    let xp_bin = paths.map_abs(Path::new("/usr/local/bin/xp"));
    if !xp_bin.exists() {
        return Err(ExitError::new(3, "xp_not_installed"));
    }

    let metadata_path = paths
        .map_abs(&args.xp_data_dir)
        .join("cluster")
        .join("metadata.json");
    if metadata_path.exists() {
        return Ok(());
    }

    if mode == Mode::DryRun {
        eprintln!("would run as user xp: /usr/local/bin/xp init ...");
        return Ok(());
    }

    if is_test_root(paths.root()) {
        return Err(ExitError::new(
            5,
            "xp_init_failed: xp bootstrap requires real system environment (use --dry-run for tests)",
        ));
    }

    // Prefer runuser if present; fallback to su.
    let has_runuser = Command::new("runuser")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let status = if has_runuser {
        let mut c = Command::new("runuser");
        c.args(["-u", "xp", "--", "/usr/local/bin/xp", "init"]);
        c.args([
            "--data-dir",
            args.xp_data_dir.to_string_lossy().as_ref(),
            "--node-name",
            &args.node_name,
            "--access-host",
            &args.access_host,
            "--api-base-url",
            &args.api_base_url,
        ]);
        c.status()
    } else {
        let cmdline = format!(
            "/usr/local/bin/xp init --data-dir {} --node-name {} --access-host {} --api-base-url {}",
            sh_quote(&args.xp_data_dir.to_string_lossy()),
            sh_quote(&args.node_name),
            sh_quote(&args.access_host),
            sh_quote(&args.api_base_url),
        );
        Command::new("su")
            .args(["-s", "/bin/sh", "xp", "-c", &cmdline])
            .status()
    };
    let status = status.map_err(|e| ExitError::new(5, format!("xp_init_failed: {e}")))?;
    if !status.success() {
        return Err(ExitError::new(5, "xp_init_failed"));
    }
    Ok(())
}

pub async fn cmd_xp_restart(paths: Paths, args: XpRestartArgs) -> Result<(), ExitError> {
    if args.dry_run {
        eprintln!(
            "would restart xp service (init-system auto): {}",
            args.service_name
        );
        return Ok(());
    }

    if is_test_root(paths.root()) {
        return Err(ExitError::new(
            5,
            "xp_restart_failed: xp restart requires real system environment (use --dry-run for tests)",
        ));
    }

    let service = args.service_name.as_str();

    // Prefer init-system auto behavior: try systemd first, then OpenRC.
    let systemd_ok = Command::new("systemctl")
        .args(["restart", format!("{service}.service").as_str()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if systemd_ok {
        return Ok(());
    }

    let openrc_ok = Command::new("rc-service")
        .args([service, "restart"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if openrc_ok {
        return Ok(());
    }

    Err(ExitError::new(
        6,
        "xp_restart_failed: failed to restart service (hint: run via sudo; ensure systemctl/rc-service exists)",
    ))
}

pub async fn cmd_xp_join(
    paths: Paths,
    xp_data_dir: std::path::PathBuf,
    node_name: String,
    access_host: String,
    api_base_url: String,
    join_token: String,
    dry_run: bool,
) -> Result<(), ExitError> {
    let mode = if dry_run { Mode::DryRun } else { Mode::Real };

    if join_token.trim().is_empty() {
        return Err(ExitError::new(2, "invalid_args: join token is empty"));
    }
    validate_https_origin(&api_base_url)?;

    let xp_bin = paths.map_abs(Path::new("/usr/local/bin/xp"));
    if !xp_bin.exists() {
        return Err(ExitError::new(3, "xp_not_installed"));
    }

    let metadata_path = paths
        .map_abs(&xp_data_dir)
        .join("cluster")
        .join("metadata.json");
    if metadata_path.exists() {
        return Ok(());
    }

    if mode == Mode::DryRun {
        eprintln!("would run as user xp: /usr/local/bin/xp join ...");
        return Ok(());
    }

    if is_test_root(paths.root()) {
        return Err(ExitError::new(
            5,
            "xp_join_failed: xp join requires real system environment (use --dry-run for tests)",
        ));
    }

    // Prefer runuser if present; fallback to su.
    let has_runuser = Command::new("runuser")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let status = if has_runuser {
        let mut c = Command::new("runuser");
        c.args(["-u", "xp", "--", "/usr/local/bin/xp", "join"]);
        c.args([
            "--data-dir",
            xp_data_dir.to_string_lossy().as_ref(),
            "--node-name",
            &node_name,
            "--access-host",
            &access_host,
            "--api-base-url",
            &api_base_url,
            "--token",
            &join_token,
        ]);
        c.status()
    } else {
        let cmdline = format!(
            "/usr/local/bin/xp join --data-dir {} --node-name {} --access-host {} --api-base-url {} --token {}",
            sh_quote(&xp_data_dir.to_string_lossy()),
            sh_quote(&node_name),
            sh_quote(&access_host),
            sh_quote(&api_base_url),
            sh_quote(&join_token),
        );
        Command::new("su")
            .args(["-s", "/bin/sh", "xp", "-c", &cmdline])
            .status()
    };
    let status = status.map_err(|e| ExitError::new(5, format!("xp_join_failed: {e}")))?;
    if !status.success() {
        return Err(ExitError::new(5, "xp_join_failed"));
    }
    Ok(())
}

fn sh_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn validate_https_origin(origin: &str) -> Result<(), ExitError> {
    let url = reqwest::Url::parse(origin)
        .map_err(|_| ExitError::new(2, "invalid_args: --api-base-url must be a valid URL"))?;
    if url.scheme() != "https" {
        return Err(ExitError::new(
            2,
            "invalid_args: --api-base-url must use https",
        ));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: --api-base-url must be an origin (no path/query)",
        ));
    }
    Ok(())
}
