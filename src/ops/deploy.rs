use crate::ops::cli::{
    DeployArgs, ExitError, InitArgs, InitSystemArg, InstallArgs, InstallOnly, XpBootstrapArgs,
    XpInstallArgs,
};
use crate::ops::cloudflare;
use crate::ops::init;
use crate::ops::install;
use crate::ops::paths::Paths;
use crate::ops::util::{Mode, chmod, ensure_dir, is_test_root, write_string_if_changed};
use crate::ops::xp;
use rand::RngCore;
use std::fs;
use std::path::Path;

pub async fn cmd_deploy(paths: Paths, args: DeployArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    let cloudflare_enabled = args.cloudflare_toggle.enabled();

    let (api_base_url, cf) = if cloudflare_enabled {
        let account_id = args.account_id.clone().ok_or_else(|| {
            ExitError::new(
                2,
                "invalid_args: --account-id is required with --cloudflare",
            )
        })?;
        let zone_id = args.zone_id.clone().ok_or_else(|| {
            ExitError::new(2, "invalid_args: --zone-id is required with --cloudflare")
        })?;
        let hostname = args.hostname.clone().ok_or_else(|| {
            ExitError::new(2, "invalid_args: --hostname is required with --cloudflare")
        })?;
        let origin_url = args.origin_url.clone().ok_or_else(|| {
            ExitError::new(
                2,
                "invalid_args: --origin-url is required with --cloudflare",
            )
        })?;

        let api_base_url = format!("https://{hostname}");
        validate_https_origin_no_port(&api_base_url)?;

        (
            api_base_url,
            Some((account_id, zone_id, hostname, origin_url)),
        )
    } else {
        let api_base_url = args.api_base_url.clone().ok_or_else(|| {
            ExitError::new(
                2,
                "invalid_args: --api-base-url is required with --no-cloudflare",
            )
        })?;
        validate_https_origin_no_port(&api_base_url)?;
        (api_base_url, None)
    };

    if mode == Mode::DryRun {
        eprintln!("deploy plan:");
        eprintln!("  - install xray");
        if cloudflare_enabled {
            eprintln!("  - install cloudflared");
        }
        eprintln!("  - init directories and service files (no enable)");
        eprintln!("  - install xp binary");
        eprintln!("  - write /etc/xp/xp.env (XP_ADMIN_TOKEN)");
        eprintln!("  - xp bootstrap (xp init)");
        if cloudflare_enabled {
            eprintln!("  - cloudflare provision");
        }
        if args.enable_services_toggle.enabled() {
            eprintln!("  - enable and start services");
        }
    }

    install::cmd_install(
        paths.clone(),
        InstallArgs {
            only: Some(InstallOnly::Xray),
            xray_version: args.xray_version.clone(),
            dry_run: mode == Mode::DryRun,
        },
    )
    .await?;

    if cloudflare_enabled {
        install::cmd_install(
            paths.clone(),
            InstallArgs {
                only: Some(InstallOnly::Cloudflared),
                xray_version: "latest".to_string(),
                dry_run: mode == Mode::DryRun,
            },
        )
        .await?;
    }

    init::cmd_init(
        paths.clone(),
        InitArgs {
            xp_work_dir: Path::new("/var/lib/xp").to_path_buf(),
            xp_data_dir: Path::new("/var/lib/xp/data").to_path_buf(),
            xray_work_dir: Path::new("/var/lib/xray").to_path_buf(),
            init_system: InitSystemArg::Auto,
            enable_services: false,
            dry_run: mode == Mode::DryRun,
        },
    )
    .await?;

    xp::cmd_xp_install(
        paths.clone(),
        XpInstallArgs {
            xp_bin: args.xp_bin.clone(),
            enable: false,
            dry_run: mode == Mode::DryRun,
        },
    )
    .await?;

    ensure_xp_env_admin_token(&paths, mode)?;

    xp::cmd_xp_bootstrap(
        paths.clone(),
        XpBootstrapArgs {
            node_name: args.node_name.clone(),
            public_domain: args.public_domain.clone(),
            api_base_url: api_base_url.clone(),
            xp_data_dir: Path::new("/var/lib/xp/data").to_path_buf(),
            dry_run: mode == Mode::DryRun,
        },
    )
    .await?;

    if let Some((account_id, zone_id, hostname, origin_url)) = cf {
        cloudflare::cmd_cloudflare_provision(
            paths.clone(),
            crate::ops::cli::CloudflareProvisionArgs {
                account_id,
                zone_id,
                hostname,
                origin_url,
                enable: args.enable_services_toggle.enabled(),
                no_enable: !args.enable_services_toggle.enabled(),
                dry_run: mode == Mode::DryRun,
            },
        )
        .await?;
    }

    if args.enable_services_toggle.enabled() {
        if mode == Mode::DryRun {
            eprintln!(
                "would enable services: xray, xp{}",
                if cloudflare_enabled {
                    ", cloudflared"
                } else {
                    ""
                }
            );
            return Ok(());
        }
        if !is_test_root(paths.root()) {
            // systemd first, then OpenRC best-effort.
            let _ = std::process::Command::new("systemctl")
                .args(["daemon-reload"])
                .status();
            let _ = std::process::Command::new("systemctl")
                .args(["enable", "--now", "xray.service"])
                .status();
            let _ = std::process::Command::new("systemctl")
                .args(["enable", "--now", "xp.service"])
                .status();
            if cloudflare_enabled {
                let _ = std::process::Command::new("systemctl")
                    .args(["enable", "--now", "cloudflared.service"])
                    .status();
            }
        }
    }

    Ok(())
}

fn ensure_xp_env_admin_token(paths: &Paths, mode: Mode) -> Result<(), ExitError> {
    let p = paths.etc_xp_env();
    if mode == Mode::DryRun {
        eprintln!("would ensure: {}", p.display());
        return Ok(());
    }

    ensure_dir(&paths.etc_xp_dir())
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;

    let existing = fs::read_to_string(&p).ok();
    if let Some(s) = existing
        && s.lines()
            .any(|l| l.starts_with("XP_ADMIN_TOKEN=") && l.len() > "XP_ADMIN_TOKEN=".len())
    {
        return Ok(());
    }

    let token = generate_admin_token();
    let content = format!("XP_ADMIN_TOKEN={token}\n");
    write_string_if_changed(&p, &content)
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    chmod(&p, 0o600).ok();
    Ok(())
}

fn generate_admin_token() -> String {
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

fn validate_https_origin_no_port(origin: &str) -> Result<(), ExitError> {
    let url =
        reqwest::Url::parse(origin).map_err(|_| ExitError::new(2, "invalid_args: invalid url"))?;
    if url.scheme() != "https" {
        return Err(ExitError::new(
            2,
            "invalid_args: api-base-url must be https",
        ));
    }
    if url.port().is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: api-base-url must not specify a custom port",
        ));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: api-base-url must be an origin (no path/query)",
        ));
    }
    Ok(())
}
