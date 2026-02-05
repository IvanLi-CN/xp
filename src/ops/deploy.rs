use crate::admin_token::{hash_admin_token_argon2id, parse_admin_token_hash, verify_admin_token};
use crate::ops::cli::{
    DeployArgs, ExitError, InitArgs, InitSystemArg, InstallArgs, InstallOnly, XpBootstrapArgs,
    XpInstallArgs,
};
use crate::ops::cloudflare::{self, CloudflareTokenSource, DnsRecordInfo, TunnelInfo, ZoneLookup};
use crate::ops::init;
use crate::ops::install;
use crate::ops::paths::Paths;
use crate::ops::platform::{InitSystem, detect_distro, detect_init_system};
use crate::ops::util::{Mode, is_test_root};
use crate::ops::xp;
use dialoguer::Confirm;
use dialoguer::Select;
use nanoid::nanoid;
use rand::RngCore;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_ORIGIN_URL: &str = "http://127.0.0.1:62416";
const HOSTNAME_SUFFIX_LEN: usize = 4;
const HOSTNAME_ALPHABET: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueSource {
    Provided,
    Derived,
    Generated,
}

#[derive(Debug, Clone)]
struct CloudflarePlan {
    account_id: String,
    zone_id: String,
    zone_id_source: ValueSource,
    zone_name: String,
    zone_name_source: ValueSource,
    hostname: String,
    hostname_source: ValueSource,
    tunnel_name: String,
    tunnel_name_source: ValueSource,
    origin_url: String,
    origin_url_source: ValueSource,
    tunnel_conflict: Option<TunnelInfo>,
    tunnel_override: Option<TunnelInfo>,
    dns_conflict: Option<DnsRecordInfo>,
    dns_override: Option<DnsRecordInfo>,
}

#[derive(Debug, Clone)]
struct DeployPlan {
    xp_install_from: Option<PathBuf>,
    xp_path: PathBuf,
    node_name: String,
    access_host: String,
    api_base_url: String,
    api_base_url_source: ValueSource,
    join_token_present: bool,
    xray_version: String,
    enable_services: bool,
    cloudflare_enabled: bool,
    cloudflare_token_source: Option<CloudflareTokenSource>,
    cloudflare: Option<CloudflarePlan>,
    warnings: Vec<String>,
    errors: Vec<String>,
}

pub async fn cmd_deploy(paths: Paths, mut args: DeployArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    let auto_yes = args.yes;
    let force_overwrite = args.overwrite_existing;
    let cloudflare_enabled = args.cloudflare_toggle.enabled();
    let interactive = !args.non_interactive && io::stdin().is_terminal();

    if args.join_token_stdin && args.cloudflare_token_stdin {
        return Err(ExitError::new(
            2,
            "invalid_args: --join-token-stdin conflicts with --cloudflare-token-stdin (stdin can only provide one secret)",
        ));
    }

    if args.join_token_stdin {
        if io::stdin().is_terminal() {
            return Err(ExitError::new(
                2,
                "invalid_args: --join-token-stdin requires piped stdin (e.g. printf \"%s\" <token> | ...)",
            ));
        }
        let mut s = String::new();
        io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| ExitError::new(2, format!("invalid_args: read stdin: {e}")))?;
        let token = s.trim().to_string();
        if token.is_empty() {
            return Err(ExitError::new(
                2,
                "invalid_args: --join-token-stdin was set but stdin was empty",
            ));
        }
        args.join_token_stdin_value = Some(token);
    }

    if args.cloudflare_token_stdin {
        if !cloudflare_enabled {
            return Err(ExitError::new(
                2,
                "invalid_args: --cloudflare-token-stdin requires Cloudflare enabled (remove --no-cloudflare)",
            ));
        }
        if io::stdin().is_terminal() {
            return Err(ExitError::new(
                2,
                "invalid_args: --cloudflare-token-stdin requires piped stdin (e.g. printf \"%s\" <token> | ...)",
            ));
        }
        let mut s = String::new();
        io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| ExitError::new(2, format!("invalid_args: read stdin: {e}")))?;
        let token = s.trim().to_string();
        if token.is_empty() {
            return Err(ExitError::new(
                3,
                "cloudflare token missing: --cloudflare-token-stdin was set but stdin was empty (use --cloudflare-token / CLOUDFLARE_API_TOKEN / /etc/xp-ops/cloudflare_tunnel/api_token)",
            ));
        }
        args.cloudflare_token_stdin_value = Some(token);
    }

    if args.cloudflare_token.is_some() && !cloudflare_enabled {
        return Err(ExitError::new(
            2,
            "invalid_args: --cloudflare-token requires Cloudflare enabled (remove --no-cloudflare)",
        ));
    }

    let mut plan = build_plan(&paths, &args).await?;
    let has_conflict = plan.cloudflare_enabled
        && plan
            .cloudflare
            .as_ref()
            .is_some_and(|cf| cf.dns_conflict.is_some() || cf.tunnel_conflict.is_some());
    let suppress_preflight = has_conflict && (interactive || auto_yes);

    if !suppress_preflight {
        render_plan(&plan);
    }

    if !plan.errors.is_empty() {
        if suppress_preflight {
            render_plan_issues(&plan, true);
        }
        return Err(ExitError::new(2, "preflight_failed: fix errors above"));
    }

    if plan.cloudflare_enabled {
        if plan.cloudflare.as_ref().unwrap().dns_conflict.is_some() {
            if force_overwrite {
                let (new_args, new_plan) =
                    force_overwrite_hostname_conflict(&paths, args, plan, mode == Mode::DryRun)
                        .await?;
                args = new_args;
                plan = new_plan;
                if !plan.errors.is_empty() {
                    return Err(ExitError::new(2, "preflight_failed: fix errors above"));
                }
            } else if auto_yes {
                let (new_args, new_plan) =
                    auto_resolve_hostname_conflict(&paths, args, plan, mode == Mode::DryRun)
                        .await?;
                args = new_args;
                plan = new_plan;
                if !plan.errors.is_empty() {
                    return Err(ExitError::new(2, "preflight_failed: fix errors above"));
                }
            } else if interactive {
                let (new_args, new_plan) =
                    resolve_hostname_conflict(&paths, args, plan, mode == Mode::DryRun).await?;
                args = new_args;
                plan = new_plan;
                if !plan.errors.is_empty() {
                    return Err(ExitError::new(2, "preflight_failed: fix errors above"));
                }
            } else {
                return Err(ExitError::new(
                    2,
                    "hostname_conflict: use -y to auto-resolve or run interactively",
                ));
            }
        }

        if plan.cloudflare.as_ref().unwrap().tunnel_conflict.is_some() {
            if force_overwrite {
                let (new_args, new_plan) =
                    force_overwrite_tunnel_conflict(&paths, args, plan, mode == Mode::DryRun)
                        .await?;
                args = new_args;
                plan = new_plan;
                if !plan.errors.is_empty() {
                    return Err(ExitError::new(2, "preflight_failed: fix errors above"));
                }
            } else if auto_yes {
                let (new_args, new_plan) =
                    auto_resolve_tunnel_conflict(&paths, args, plan, mode == Mode::DryRun).await?;
                args = new_args;
                plan = new_plan;
                if !plan.errors.is_empty() {
                    return Err(ExitError::new(2, "preflight_failed: fix errors above"));
                }
            } else if interactive {
                let (new_args, new_plan) =
                    resolve_tunnel_conflict(&paths, args, plan, mode == Mode::DryRun).await?;
                args = new_args;
                plan = new_plan;
                if !plan.errors.is_empty() {
                    return Err(ExitError::new(2, "preflight_failed: fix errors above"));
                }
            } else {
                return Err(ExitError::new(
                    2,
                    "tunnel_conflict: use -y to auto-resolve or run interactively",
                ));
            }
        }
    }

    if suppress_preflight {
        render_plan(&plan);
    }

    if interactive && !auto_yes && !confirm("Proceed with deploy? [y/N]: ")? {
        return Ok(());
    }

    if mode == Mode::DryRun {
        eprintln!("deploy plan:");
        eprintln!("  - install xray");
        if plan.cloudflare_enabled {
            eprintln!("  - install cloudflared");
        }
        eprintln!("  - init directories and service files (no enable)");
        eprintln!("  - install xp binary");
        if plan.join_token_present {
            eprintln!("  - xp join (cluster join token)");
            eprintln!("  - write /etc/xp/xp.env (XP_ADMIN_TOKEN_HASH)");
        } else {
            eprintln!("  - write /etc/xp/xp.env (XP_ADMIN_TOKEN_HASH; print token once)");
            eprintln!("  - xp bootstrap (xp init)");
        }
        if plan.cloudflare_enabled {
            eprintln!("  - cloudflare provision");
        }
        if plan.enable_services {
            eprintln!("  - enable and start services");
        }
    }

    install::cmd_install(
        paths.clone(),
        InstallArgs {
            only: Some(InstallOnly::Xray),
            xray_version: plan.xray_version.clone(),
            dry_run: mode == Mode::DryRun,
        },
    )
    .await?;

    if plan.cloudflare_enabled {
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

    // Run `xp-ops init` first so the `xp` group (and related init-system files) exist;
    // then write /etc/xp/xp.env (XP_ADMIN_TOKEN_HASH) with correct ownership.
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

    // After `xp-ops init`, we know the `xp` group exists (so `chown root:xp` is reliable).
    let bootstrap_admin_token = if plan.join_token_present {
        None
    } else {
        ensure_xp_env_admin_token_hash_bootstrap(
            &paths,
            mode,
            plan.node_name.as_str(),
            plan.access_host.as_str(),
            plan.api_base_url.as_str(),
            force_overwrite,
        )?
    };
    if mode == Mode::Real
        && !is_test_root(paths.root())
        && let Some(token) = bootstrap_admin_token.as_ref()
    {
        eprintln!("admin token (save it now; printed once; not stored on the server):");
        println!("{token}");
    }

    if let Some(xp_bin) = plan.xp_install_from.clone() {
        xp::cmd_xp_install(
            paths.clone(),
            XpInstallArgs {
                xp_bin,
                enable: false,
                dry_run: mode == Mode::DryRun,
            },
        )
        .await?;
    }

    if plan.join_token_present {
        let join_token = args
            .join_token
            .clone()
            .or(args.join_token_stdin_value.clone())
            .ok_or_else(|| ExitError::new(2, "invalid_args: join token is missing"))?;

        xp::cmd_xp_join(
            paths.clone(),
            "/var/lib/xp/data".into(),
            plan.node_name.clone(),
            plan.access_host.clone(),
            plan.api_base_url.clone(),
            join_token,
            mode == Mode::DryRun,
        )
        .await?;

        if mode == Mode::DryRun {
            // `xp join` dry-run does not materialize cluster metadata/hash files.
            eprintln!("would write /etc/xp/xp.env (XP_ADMIN_TOKEN_HASH) from join result");
        } else {
            let hash = read_cluster_admin_token_hash(&paths, Path::new("/var/lib/xp/data"))?;
            ensure_xp_env_admin_token_hash_join(
                &paths,
                mode,
                &hash,
                plan.node_name.as_str(),
                plan.access_host.as_str(),
                plan.api_base_url.as_str(),
                force_overwrite,
            )?;
        }
    } else {
        xp::cmd_xp_bootstrap(
            paths.clone(),
            XpBootstrapArgs {
                node_name: plan.node_name.clone(),
                access_host: plan.access_host.clone(),
                api_base_url: plan.api_base_url.clone(),
                xp_data_dir: Path::new("/var/lib/xp/data").to_path_buf(),
                dry_run: mode == Mode::DryRun,
            },
        )
        .await?;
    }

    if let Some(cf) = plan.cloudflare.clone() {
        let token = cloudflare::load_cloudflare_token_for_deploy(
            &paths,
            args.cloudflare_token.as_deref(),
            args.cloudflare_token_stdin_value.as_deref(),
        )
        .map(|(t, _src)| t)
        .map_err(|e| {
            if e.message == "token_missing" {
                ExitError::new(
                    3,
                    "cloudflare token missing: provide --cloudflare-token / --cloudflare-token-stdin, or set CLOUDFLARE_API_TOKEN, or write /etc/xp-ops/cloudflare_tunnel/api_token",
                )
            } else {
                e
            }
        })?;

        // Ensure deploy uses the exact token resolved for this run (incl. --cloudflare-token-stdin).
        // `cmd_cloudflare_provision` remains the standalone CLI entry which reads from env/file.
        cloudflare::cmd_cloudflare_provision_with_token(
            paths.clone(),
            crate::ops::cli::CloudflareProvisionArgs {
                tunnel_name: Some(cf.tunnel_name),
                account_id: cf.account_id,
                zone_id: cf.zone_id,
                hostname: cf.hostname,
                origin_url: cf.origin_url,
                dns_record_id_override: cf.dns_override.map(|r| r.id),
                tunnel_id_override: cf.tunnel_override.map(|t| t.id),
                enable: plan.enable_services,
                no_enable: !plan.enable_services,
                dry_run: mode == Mode::DryRun,
            },
            token,
        )
        .await?;
    }

    if plan.enable_services {
        if mode == Mode::DryRun {
            eprintln!(
                "would enable services: xray, xp{}",
                if plan.cloudflare_enabled {
                    ", cloudflared"
                } else {
                    ""
                }
            );
            return Ok(());
        }
        if !is_test_root(paths.root()) {
            let distro = detect_distro(&paths).map_err(|e| ExitError::new(2, e))?;
            let init_system = detect_init_system(distro, None);

            match init_system {
                InitSystem::Systemd => {
                    let _ = std::process::Command::new("systemctl")
                        .args(["daemon-reload"])
                        .status();
                    let _ = std::process::Command::new("systemctl")
                        .args(["enable", "--now", "xray.service"])
                        .status();
                    let _ = std::process::Command::new("systemctl")
                        .args(["enable", "--now", "xp.service"])
                        .status();
                    if plan.cloudflare_enabled {
                        let _ = std::process::Command::new("systemctl")
                            .args(["enable", "--now", "cloudflared.service"])
                            .status();
                    }
                }
                InitSystem::OpenRc => {
                    let _ = std::process::Command::new("rc-update")
                        .args(["add", "xray", "default"])
                        .status();
                    let _ = std::process::Command::new("rc-update")
                        .args(["add", "xp", "default"])
                        .status();
                    let _ = std::process::Command::new("rc-service")
                        .args(["xray", "start"])
                        .status();
                    let _ = std::process::Command::new("rc-service")
                        .args(["xp", "start"])
                        .status();
                    if plan.cloudflare_enabled {
                        let _ = std::process::Command::new("rc-update")
                            .args(["add", "cloudflared", "default"])
                            .status();
                        let _ = std::process::Command::new("rc-service")
                            .args(["cloudflared", "start"])
                            .status();
                    }
                }
                InitSystem::None => {}
            }

            wait_for_service(init_system, "xray").await?;
            wait_for_service(init_system, "xp").await?;
            if plan.cloudflare_enabled {
                wait_for_service(init_system, "cloudflared").await?;
            }
        }
    }

    if mode == Mode::Real
        && plan.cloudflare_enabled
        && plan.cloudflare_token_source == Some(CloudflareTokenSource::Flag)
    {
        eprintln!(
            "security note: Cloudflare API token was provided via --cloudflare-token; this may leak via shell history or process list. Rotate/revoke it after this deploy."
        );
    }

    if mode == Mode::Real && !is_test_root(paths.root()) {
        eprintln!(
            "admin note: XP admin token is printed once during bootstrap deploy; the server only stores its hash in /etc/xp/xp.env (XP_ADMIN_TOKEN_HASH)."
        );
    }

    Ok(())
}

async fn build_plan(paths: &Paths, args: &DeployArgs) -> Result<DeployPlan, ExitError> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    let join_token_present = args.join_token.is_some() || args.join_token_stdin_value.is_some();

    let xp_path = paths.usr_local_bin_xp();
    match args.xp_bin.as_ref() {
        Some(v) => {
            if !v.exists() {
                errors.push("xp-bin does not exist".to_string());
            }
        }
        None => {
            if !xp_path.exists() {
                errors.push(format!(
                    "xp is not installed: {} (install xp first, or pass --xp-bin)",
                    xp_path.display()
                ));
            }
        }
    }

    if args.access_host.trim().is_empty() {
        warnings.push("access_host is empty".to_string());
    }

    let cloudflare_enabled = args.cloudflare_toggle.enabled();
    let mut api_base_url_source = ValueSource::Provided;
    let mut cloudflare_plan = None;
    let mut cloudflare_token_source: Option<CloudflareTokenSource> = None;

    let api_base_url = if cloudflare_enabled {
        let account_id = match args.account_id.clone() {
            Some(v) => v,
            None => {
                errors.push("missing --account-id with --cloudflare".to_string());
                String::new()
            }
        };

        let token = match cloudflare::load_cloudflare_token_for_deploy(
            paths,
            args.cloudflare_token.as_deref(),
            args.cloudflare_token_stdin_value.as_deref(),
        ) {
            Ok((v, src)) => {
                cloudflare_token_source = Some(src);
                Some(v)
            }
            Err(e) => {
                if e.message == "token_missing" {
                    errors.push(
                        "cloudflare token missing: provide --cloudflare-token / --cloudflare-token-stdin, or set CLOUDFLARE_API_TOKEN, or write /etc/xp-ops/cloudflare_tunnel/api_token"
                            .to_string(),
                    );
                } else {
                    errors.push(format!("cloudflare token error: {}", e.message));
                }
                None
            }
        };

        let api_base = cloudflare::cloudflare_api_base();
        let mut zone_id = args.zone_id.clone();
        let mut zone_id_source = ValueSource::Provided;
        let mut zone_name = String::new();
        let zone_name_source = ValueSource::Derived;
        let mut zone_account_id: Option<String> = None;

        if zone_id.is_none() {
            zone_id_source = ValueSource::Derived;
            if let Some(domain) = zone_lookup_domain(args) {
                if let Some(token) = token.as_ref() {
                    match resolve_zone_from_domain(
                        &api_base,
                        token,
                        account_id.as_str(),
                        domain.as_str(),
                        &mut warnings,
                    )
                    .await
                    {
                        Ok(Some(found)) => {
                            zone_id = Some(found.id.clone());
                            zone_name = found.name.clone();
                            zone_account_id = found.account_id.clone();
                        }
                        Ok(None) => {
                            errors.push(format!(
                                "cloudflare zone error: no zone found for domain {domain}"
                            ));
                        }
                        Err(e) => {
                            errors.push(format!("cloudflare zone error: {}", e.message));
                        }
                    }
                }
            } else {
                errors.push(
                    "missing --zone-id with --cloudflare (provide --hostname or --zone-id)"
                        .to_string(),
                );
            }
        }

        if zone_name.is_empty()
            && let (Some(token), Some(id)) = (token.as_deref(), zone_id.as_deref())
        {
            match cloudflare::fetch_zone_info(&api_base, token, id).await {
                Ok(info) => {
                    zone_name = info.name;
                    zone_account_id = info.account_id;
                }
                Err(e) => {
                    errors.push(format!("cloudflare zone error: {}", e.message));
                }
            }
        }

        if let Some(zone_account) = zone_account_id.as_ref()
            && !account_id.is_empty()
            && &account_id != zone_account
        {
            warnings.push(format!(
                "account-id does not match zone account (zone account: {zone_account})"
            ));
        }

        let (hostname, hostname_source) = if let Some(h) = args.hostname.clone() {
            (h, ValueSource::Provided)
        } else if !zone_name.is_empty() {
            match derive_hostname(&args.node_name, &zone_name, &mut warnings, &mut errors) {
                Some(h) => (h, ValueSource::Derived),
                None => (String::new(), ValueSource::Derived),
            }
        } else {
            errors.push("cannot derive hostname without zone name".to_string());
            (String::new(), ValueSource::Derived)
        };

        if !hostname.is_empty() && !is_valid_hostname(&hostname) {
            errors.push("hostname is not a valid DNS name".to_string());
        }

        if !zone_name.is_empty() && !hostname.is_empty() && !hostname_in_zone(&hostname, &zone_name)
        {
            warnings.push(format!("hostname does not belong to zone {zone_name}"));
        }

        let api_base_url = if hostname.is_empty() {
            String::new()
        } else {
            api_base_url_source = ValueSource::Derived;
            format!("https://{hostname}")
        };
        if !api_base_url.is_empty()
            && let Err(e) = validate_https_origin_no_port(&api_base_url)
        {
            errors.push(e.message);
        }

        let (origin_url, origin_source) = match args.origin_url.clone() {
            Some(v) => (v, ValueSource::Provided),
            None => (DEFAULT_ORIGIN_URL.to_string(), ValueSource::Generated),
        };

        let (tunnel_name, tunnel_source) = match args.tunnel_name.clone() {
            Some(v) => (v, ValueSource::Provided),
            None => (
                format!("xp-{}", args.node_name.trim()),
                ValueSource::Derived,
            ),
        };

        let tunnel_conflict = if let (Some(token), true) = (
            token.as_ref(),
            !tunnel_name.trim().is_empty() && !account_id.trim().is_empty(),
        ) {
            match cloudflare::find_tunnel_by_name(&api_base, token, &account_id, &tunnel_name).await
            {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("cloudflare tunnel error: {}", e.message));
                    None
                }
            }
        } else {
            None
        };
        if let Some(tunnel) = tunnel_conflict.as_ref() {
            warnings.push(format!(
                "tunnel name already exists: {} ({})",
                tunnel.name, tunnel.id
            ));
        }

        let zone_id_value = zone_id.clone().unwrap_or_default();
        let dns_conflict = if let (Some(token), true) = (
            token.as_ref(),
            !hostname.is_empty() && !zone_id_value.is_empty(),
        ) {
            match cloudflare::find_dns_record(&api_base, token, &zone_id_value, &hostname).await {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("cloudflare dns error: {}", e.message));
                    None
                }
            }
        } else {
            None
        };
        if let Some(rec) = dns_conflict.as_ref() {
            warnings.push(format!(
                "hostname already exists: {} {} -> {}",
                rec.record_type, rec.name, rec.content
            ));
        }

        cloudflare_plan = Some(CloudflarePlan {
            account_id,
            zone_id: zone_id_value,
            zone_id_source,
            zone_name,
            zone_name_source,
            hostname,
            hostname_source,
            tunnel_name,
            tunnel_name_source: tunnel_source,
            origin_url,
            origin_url_source: origin_source,
            tunnel_conflict,
            tunnel_override: None,
            dns_conflict,
            dns_override: None,
        });
        api_base_url
    } else {
        let base = match args.api_base_url.clone() {
            Some(v) => v,
            None => {
                errors.push("missing --api-base-url with --no-cloudflare".to_string());
                String::new()
            }
        };
        if !base.is_empty()
            && let Err(e) = validate_https_origin_no_port(&base)
        {
            errors.push(e.message);
        }
        base
    };

    Ok(DeployPlan {
        xp_install_from: args.xp_bin.clone(),
        xp_path,
        node_name: args.node_name.clone(),
        access_host: args.access_host.clone(),
        api_base_url,
        api_base_url_source,
        join_token_present,
        xray_version: args.xray_version.clone(),
        enable_services: args.enable_services_toggle.enabled(),
        cloudflare_enabled,
        cloudflare_token_source,
        cloudflare: cloudflare_plan,
        warnings,
        errors,
    })
}

async fn resolve_hostname_conflict(
    paths: &Paths,
    mut args: DeployArgs,
    mut plan: DeployPlan,
    dry_run: bool,
) -> Result<(DeployArgs, DeployPlan), ExitError> {
    loop {
        let cf = plan.cloudflare.clone().unwrap();
        if cf.dns_conflict.is_none() {
            return Ok((args, plan));
        }

        eprintln!("{}", warn("hostname already exists; choose how to proceed"));
        let hostname_provided = matches!(cf.hostname_source, ValueSource::Provided);
        let mut options = Vec::new();
        options.push("input a new hostname".to_string());
        if !hostname_provided {
            options.push("input a new node-name".to_string());
        }
        options.push("auto-generate hostname".to_string());
        let overwrite_idx = if is_overwritable_record(cf.dns_conflict.as_ref()) {
            let idx = options.len();
            options.push("overwrite existing DNS record".to_string());
            Some(idx)
        } else {
            None
        };
        options.push("cancel deploy".to_string());

        let choice = select_menu(&options)?;
        let mut auto_generated = false;

        if hostname_provided {
            match choice {
                0 => {
                    let h = prompt("New hostname: ")?;
                    args.hostname = Some(h.trim().to_string());
                }
                1 => {
                    let h = generate_hostname(&args.node_name, cf.zone_name.as_str())?;
                    args.hostname = Some(h);
                    auto_generated = true;
                }
                2 if overwrite_idx == Some(2) => {
                    if confirm_overwrite(cf.dns_conflict.as_ref())? {
                        let rec = cf.dns_conflict.clone().unwrap();
                        let cf_plan = plan.cloudflare.as_mut().unwrap();
                        cf_plan.dns_override = Some(rec.clone());
                        cf_plan.dns_conflict = None;
                        let warning = format!(
                            "will overwrite existing DNS record: {} {} -> {}",
                            rec.record_type, rec.name, rec.content
                        );
                        plan.warnings
                            .retain(|w| !w.starts_with("hostname already exists:"));
                        plan.warnings.push(warning);
                        return Ok((args, plan));
                    }
                }
                2 => return Err(ExitError::new(2, "deploy_cancelled")),
                3 if overwrite_idx == Some(2) => return Err(ExitError::new(2, "deploy_cancelled")),
                _ => continue,
            }
        } else {
            match choice {
                0 => {
                    let h = prompt("New hostname: ")?;
                    args.hostname = Some(h.trim().to_string());
                }
                1 => {
                    let n = prompt("New node-name: ")?;
                    args.node_name = n.trim().to_string();
                    args.hostname = None;
                }
                2 => {
                    let h = generate_hostname(&args.node_name, cf.zone_name.as_str())?;
                    args.hostname = Some(h);
                    auto_generated = true;
                }
                3 if overwrite_idx == Some(3) => {
                    if confirm_overwrite(cf.dns_conflict.as_ref())? {
                        let rec = cf.dns_conflict.clone().unwrap();
                        let cf_plan = plan.cloudflare.as_mut().unwrap();
                        cf_plan.dns_override = Some(rec.clone());
                        cf_plan.dns_conflict = None;
                        let warning = format!(
                            "will overwrite existing DNS record: {} {} -> {}",
                            rec.record_type, rec.name, rec.content
                        );
                        plan.warnings
                            .retain(|w| !w.starts_with("hostname already exists:"));
                        plan.warnings.push(warning);
                        return Ok((args, plan));
                    }
                }
                3 => return Err(ExitError::new(2, "deploy_cancelled")),
                4 if overwrite_idx == Some(3) => return Err(ExitError::new(2, "deploy_cancelled")),
                _ => continue,
            }
        }

        plan = build_plan(paths, &args).await?;
        if auto_generated && let Some(cf_plan) = plan.cloudflare.as_mut() {
            cf_plan.hostname_source = ValueSource::Generated;
        }

        if dry_run {
            return Ok((args, plan));
        }
    }
}

async fn auto_resolve_hostname_conflict(
    paths: &Paths,
    mut args: DeployArgs,
    mut plan: DeployPlan,
    dry_run: bool,
) -> Result<(DeployArgs, DeployPlan), ExitError> {
    let mut attempts = 0;
    loop {
        let cf = plan.cloudflare.as_ref().unwrap();
        if cf.dns_conflict.is_none() {
            return Ok((args, plan));
        }
        if attempts >= 5 {
            return Err(ExitError::new(
                2,
                "hostname_conflict: unable to auto-resolve after 5 attempts",
            ));
        }
        let h = generate_hostname(&args.node_name, cf.zone_name.as_str())?;
        args.hostname = Some(h);
        let mut new_plan = build_plan(paths, &args).await?;
        if let Some(cf_plan) = new_plan.cloudflare.as_mut() {
            cf_plan.hostname_source = ValueSource::Generated;
        }
        plan = new_plan;
        if dry_run {
            return Ok((args, plan));
        }
        attempts += 1;
    }
}

async fn force_overwrite_hostname_conflict(
    _paths: &Paths,
    args: DeployArgs,
    mut plan: DeployPlan,
    _dry_run: bool,
) -> Result<(DeployArgs, DeployPlan), ExitError> {
    let cf = plan.cloudflare.clone().unwrap();
    let Some(rec) = cf.dns_conflict.clone() else {
        return Ok((args, plan));
    };
    if !is_overwritable_record(Some(&rec)) {
        return Err(ExitError::new(
            2,
            format!(
                "hostname_conflict: record type {} cannot be overwritten",
                rec.record_type
            ),
        ));
    }
    let cf_plan = plan.cloudflare.as_mut().unwrap();
    cf_plan.dns_override = Some(rec.clone());
    cf_plan.dns_conflict = None;
    plan.warnings
        .retain(|w| !w.starts_with("hostname already exists:"));
    plan.warnings.push(format!(
        "will overwrite existing DNS record: {} {} -> {}",
        rec.record_type, rec.name, rec.content
    ));
    Ok((args, plan))
}

async fn resolve_tunnel_conflict(
    paths: &Paths,
    mut args: DeployArgs,
    mut plan: DeployPlan,
    dry_run: bool,
) -> Result<(DeployArgs, DeployPlan), ExitError> {
    loop {
        let cf = plan.cloudflare.clone().unwrap();
        let Some(conflict) = cf.tunnel_conflict.clone() else {
            return Ok((args, plan));
        };

        eprintln!(
            "{}",
            warn("tunnel name already exists; choose how to proceed")
        );
        eprintln!(
            "{}",
            warn(&format!(
                "current tunnel: {} ({})",
                conflict.name, conflict.id
            ))
        );

        let options = vec![
            "overwrite existing tunnel".to_string(),
            "input a new tunnel name".to_string(),
            "auto-generate tunnel name".to_string(),
            "cancel deploy".to_string(),
        ];

        let choice = select_menu(&options)?;
        let mut auto_generated = false;

        match choice {
            0 => {
                if confirm_overwrite_tunnel(&conflict)? {
                    let cred_path = tunnel_credentials_path(paths, &conflict.id);
                    if !cred_path.exists() {
                        if dry_run {
                            plan.warnings.push(format!(
                                "missing tunnel credentials file: {}",
                                cred_path.display()
                            ));
                        } else {
                            return Err(ExitError::new(
                                2,
                                format!(
                                    "tunnel_conflict: missing credentials file {}",
                                    cred_path.display()
                                ),
                            ));
                        }
                    }
                    let cf_plan = plan.cloudflare.as_mut().unwrap();
                    cf_plan.tunnel_override = Some(conflict.clone());
                    cf_plan.tunnel_conflict = None;
                    plan.warnings
                        .retain(|w| !w.starts_with("tunnel name already exists:"));
                    plan.warnings.push(format!(
                        "will reuse existing tunnel: {} ({})",
                        conflict.name, conflict.id
                    ));
                    return Ok((args, plan));
                }
            }
            1 => {
                let name = prompt("New tunnel name: ")?;
                args.tunnel_name = Some(name.trim().to_string());
            }
            2 => {
                let name = generate_tunnel_name(&cf.tunnel_name);
                args.tunnel_name = Some(name);
                auto_generated = true;
            }
            3 => return Err(ExitError::new(2, "deploy_cancelled")),
            _ => continue,
        }

        plan = build_plan(paths, &args).await?;
        if auto_generated && let Some(cf_plan) = plan.cloudflare.as_mut() {
            cf_plan.tunnel_name_source = ValueSource::Generated;
        }

        if dry_run {
            return Ok((args, plan));
        }
    }
}

async fn force_overwrite_tunnel_conflict(
    paths: &Paths,
    args: DeployArgs,
    mut plan: DeployPlan,
    dry_run: bool,
) -> Result<(DeployArgs, DeployPlan), ExitError> {
    let cf = plan.cloudflare.clone().unwrap();
    let Some(conflict) = cf.tunnel_conflict.clone() else {
        return Ok((args, plan));
    };

    let cred_path = tunnel_credentials_path(paths, &conflict.id);
    if !cred_path.exists() {
        if dry_run {
            plan.warnings.push(format!(
                "missing tunnel credentials file: {}",
                cred_path.display()
            ));
        } else {
            return Err(ExitError::new(
                2,
                format!(
                    "tunnel_conflict: missing credentials file {}",
                    cred_path.display()
                ),
            ));
        }
    }

    let cf_plan = plan.cloudflare.as_mut().unwrap();
    cf_plan.tunnel_override = Some(conflict.clone());
    cf_plan.tunnel_conflict = None;
    plan.warnings
        .retain(|w| !w.starts_with("tunnel name already exists:"));
    plan.warnings.push(format!(
        "will reuse existing tunnel: {} ({})",
        conflict.name, conflict.id
    ));
    Ok((args, plan))
}

async fn auto_resolve_tunnel_conflict(
    paths: &Paths,
    mut args: DeployArgs,
    mut plan: DeployPlan,
    dry_run: bool,
) -> Result<(DeployArgs, DeployPlan), ExitError> {
    let mut attempts = 0;
    loop {
        let cf = plan.cloudflare.as_ref().unwrap();
        if cf.tunnel_conflict.is_none() {
            return Ok((args, plan));
        }
        if attempts >= 5 {
            return Err(ExitError::new(
                2,
                "tunnel_conflict: unable to auto-resolve after 5 attempts",
            ));
        }
        let name = generate_tunnel_name(&cf.tunnel_name);
        args.tunnel_name = Some(name);
        let mut new_plan = build_plan(paths, &args).await?;
        if let Some(cf_plan) = new_plan.cloudflare.as_mut() {
            cf_plan.tunnel_name_source = ValueSource::Generated;
        }
        plan = new_plan;
        if dry_run {
            return Ok((args, plan));
        }
        attempts += 1;
    }
}

fn derive_hostname(
    node_name: &str,
    zone_name: &str,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> Option<String> {
    let (label, changed) = sanitize_label(node_name);
    if label.is_empty() {
        errors.push("node-name cannot be converted to a valid hostname label".to_string());
        return None;
    }
    if changed {
        warnings.push("node-name adjusted to fit DNS label rules".to_string());
    }
    Some(format!("{label}.{zone_name}"))
}

fn generate_hostname(node_name: &str, zone_name: &str) -> Result<String, ExitError> {
    if zone_name.trim().is_empty() {
        return Err(ExitError::new(2, "missing zone name for hostname"));
    }
    let (base, _) = sanitize_label(node_name);
    let suffix = nanoid!(HOSTNAME_SUFFIX_LEN, HOSTNAME_ALPHABET);
    let label = if base.is_empty() {
        suffix
    } else {
        let trimmed = if base.len() + 1 + HOSTNAME_SUFFIX_LEN > 63 {
            &base[..(63 - 1 - HOSTNAME_SUFFIX_LEN)]
        } else {
            &base
        };
        format!("{trimmed}-{suffix}")
    };
    Ok(format!("{label}.{zone_name}"))
}

fn generate_tunnel_name(current: &str) -> String {
    let (base, _) = sanitize_label(current);
    let suffix = nanoid!(HOSTNAME_SUFFIX_LEN, HOSTNAME_ALPHABET);
    if base.is_empty() {
        format!("xp-{suffix}")
    } else {
        format!("{base}-{suffix}")
    }
}

fn sanitize_label(input: &str) -> (String, bool) {
    let mut out = String::new();
    let mut changed = false;
    for ch in input.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c);
        } else {
            out.push('-');
            changed = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed != out {
        changed = true;
    }
    (trimmed, changed)
}

fn is_valid_hostname(name: &str) -> bool {
    if name.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels.is_empty() {
        return false;
    }
    for label in labels {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        let bytes = label.as_bytes();
        if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
            return false;
        }
        if !label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return false;
        }
    }
    true
}

fn hostname_in_zone(hostname: &str, zone: &str) -> bool {
    hostname == zone || hostname.ends_with(&format!(".{zone}"))
}

fn zone_lookup_domain(args: &DeployArgs) -> Option<String> {
    if let Some(h) = args.hostname.as_ref()
        && !h.trim().is_empty()
    {
        return Some(h.trim().to_ascii_lowercase());
    }
    None
}

fn zone_name_candidates(domain: &str) -> Vec<String> {
    let trimmed = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let parts: Vec<&str> = trimmed.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..parts.len() {
        let candidate = parts[i..].join(".");
        if !candidate.is_empty() {
            out.push(candidate);
        }
    }
    out
}

async fn resolve_zone_from_domain(
    api_base: &str,
    token: &str,
    account_id: &str,
    domain: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<ZoneLookup>, ExitError> {
    let candidates = zone_name_candidates(domain);
    if candidates.is_empty() {
        return Err(ExitError::new(
            2,
            "invalid_args: domain is empty for zone lookup",
        ));
    }
    for name in candidates {
        let mut zones = cloudflare::find_zone_by_name(api_base, token, &name).await?;
        if zones.is_empty() {
            continue;
        }
        if !account_id.trim().is_empty() {
            let filtered: Vec<ZoneLookup> = zones
                .iter()
                .filter(|z| z.account_id.as_deref() == Some(account_id))
                .cloned()
                .collect();
            if filtered.is_empty() {
                warnings.push(format!(
                    "zone {name} found but account-id does not match; specify --zone-id to override"
                ));
                continue;
            }
            zones = filtered;
        }
        if zones.len() == 1 {
            return Ok(Some(zones[0].clone()));
        }
        return Err(ExitError::new(
            2,
            format!("multiple zones matched for {name}; specify --zone-id"),
        ));
    }
    Ok(None)
}

fn render_plan(plan: &DeployPlan) {
    eprintln!("preflight config:");
    let mut idx = 1;
    let mut line = |label: &str, value: String| {
        eprintln!("{idx}) {label}: {value}");
        idx += 1;
    };

    line(
        "xp",
        match plan.xp_install_from.as_ref() {
            Some(src) => {
                if src == &plan.xp_path {
                    format!("use existing {}", plan.xp_path.display())
                } else {
                    format!("install {} -> {}", src.display(), plan.xp_path.display())
                }
            }
            None => {
                if plan.xp_path.exists() {
                    format!("use existing {}", plan.xp_path.display())
                } else {
                    format!("missing {}", plan.xp_path.display())
                }
            }
        },
    );
    line("node_name", plan.node_name.clone());
    line("access_host", plan.access_host.clone());
    line(
        "cloudflare",
        if plan.cloudflare_enabled {
            "enabled"
        } else {
            "disabled"
        }
        .to_string(),
    );
    if plan.cloudflare_enabled {
        let value = match plan.cloudflare_token_source {
            Some(src) => format!("provided via {}", src.display()),
            None => "absent".to_string(),
        };
        line("cloudflare_token", value);
    }

    if let Some(cf) = plan.cloudflare.as_ref() {
        line("account_id", cf.account_id.clone());
        line("zone_id", auto(cf.zone_id.as_str(), cf.zone_id_source));
        line(
            "zone_name",
            auto(cf.zone_name.as_str(), cf.zone_name_source),
        );
        line("hostname", auto(cf.hostname.as_str(), cf.hostname_source));
        line(
            "tunnel_name",
            auto(cf.tunnel_name.as_str(), cf.tunnel_name_source),
        );
        line(
            "origin_url",
            auto(cf.origin_url.as_str(), cf.origin_url_source),
        );
    }

    line(
        "api_base_url",
        auto(plan.api_base_url.as_str(), plan.api_base_url_source),
    );
    line("xray_version", plan.xray_version.clone());
    line(
        "enable_services",
        if plan.enable_services {
            "true"
        } else {
            "false"
        }
        .to_string(),
    );

    if !plan.warnings.is_empty() {
        eprintln!("{}", warn("warnings:"));
        for w in &plan.warnings {
            eprintln!("  - {}", warn(w));
        }
    }

    if !plan.errors.is_empty() {
        eprintln!("{}", err("errors:"));
        for e in &plan.errors {
            eprintln!("  - {}", err(e));
        }
    }
}

async fn wait_for_service(init_system: InitSystem, name: &str) -> Result<(), ExitError> {
    if init_system == InitSystem::None {
        return Ok(());
    }

    let service = match init_system {
        InitSystem::Systemd => format!("{name}.service"),
        InitSystem::OpenRc | InitSystem::None => name.to_string(),
    };

    for _ in 0..10 {
        let ok = match init_system {
            InitSystem::Systemd => std::process::Command::new("systemctl")
                .args(["is-active", "--quiet", service.as_str()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false),
            InitSystem::OpenRc => std::process::Command::new("rc-service")
                .args([name, "status"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false),
            InitSystem::None => true,
        };
        if ok {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Err(ExitError::new(
        6,
        format!("service_failed: {name} is not running"),
    ))
}

fn render_plan_issues(plan: &DeployPlan, suppress_conflicts: bool) {
    if !plan.warnings.is_empty() {
        eprintln!("{}", warn("warnings:"));
        for w in &plan.warnings {
            if suppress_conflicts
                && (w.starts_with("hostname already exists:")
                    || w.starts_with("tunnel name already exists:"))
            {
                continue;
            }
            eprintln!("  - {}", warn(w));
        }
    }

    if !plan.errors.is_empty() {
        eprintln!("{}", err("errors:"));
        for e in &plan.errors {
            eprintln!("  - {}", err(e));
        }
    }
}

fn auto(value: &str, source: ValueSource) -> String {
    let label = match source {
        ValueSource::Provided => value.to_string(),
        ValueSource::Derived => bold(value),
        ValueSource::Generated => bold(value),
    };
    match source {
        ValueSource::Provided => label,
        ValueSource::Derived => format!("{label} (derived)"),
        ValueSource::Generated => format!("{label} (auto-generated)"),
    }
}

fn bold(text: &str) -> String {
    format!("\x1b[1m{text}\x1b[0m")
}

fn warn(text: &str) -> String {
    format!("\x1b[33m{text}\x1b[0m")
}

fn err(text: &str) -> String {
    format!("\x1b[31m{text}\x1b[0m")
}

fn is_overwritable_record(rec: Option<&DnsRecordInfo>) -> bool {
    matches!(
        rec.map(|r| r.record_type.as_str()),
        Some("A") | Some("AAAA") | Some("CNAME")
    )
}

fn confirm_overwrite(rec: Option<&DnsRecordInfo>) -> Result<bool, ExitError> {
    let Some(rec) = rec else {
        return Ok(false);
    };
    eprintln!(
        "{}",
        warn(&format!(
            "current record: {} {} -> {}",
            rec.record_type, rec.name, rec.content
        ))
    );
    Confirm::new()
        .with_prompt("Overwrite existing DNS record?")
        .default(false)
        .interact()
        .map_err(|e| ExitError::new(2, format!("invalid_input: {e}")))
}

fn confirm_overwrite_tunnel(tunnel: &TunnelInfo) -> Result<bool, ExitError> {
    Confirm::new()
        .with_prompt(format!(
            "Reuse existing tunnel {} ({}) and overwrite its config?",
            tunnel.name, tunnel.id
        ))
        .default(false)
        .interact()
        .map_err(|e| ExitError::new(2, format!("invalid_input: {e}")))
}

fn tunnel_credentials_path(paths: &Paths, tunnel_id: &str) -> PathBuf {
    paths
        .etc_cloudflared_dir()
        .join(format!("{tunnel_id}.json"))
}

fn select_menu(options: &[String]) -> Result<usize, ExitError> {
    if options.is_empty() {
        return Err(ExitError::new(2, "invalid_input: no options"));
    }
    let selection = Select::new()
        .with_prompt("Choose how to proceed")
        .items(options)
        .default(0)
        .interact()
        .map_err(|e| ExitError::new(2, format!("invalid_input: {e}")))?;
    Ok(selection)
}

fn prompt(message: &str) -> Result<String, ExitError> {
    print!("{message}");
    io::stdout().flush().ok();
    let mut s = String::new();
    io::stdin()
        .read_line(&mut s)
        .map_err(|e| ExitError::new(2, format!("invalid_input: {e}")))?;
    Ok(s)
}

fn confirm(message: &str) -> Result<bool, ExitError> {
    let resp = prompt(message)?;
    let v = resp.trim().to_ascii_lowercase();
    Ok(v == "y" || v == "yes")
}

fn ensure_xp_env_admin_token_hash_bootstrap(
    paths: &Paths,
    mode: Mode,
    node_name: &str,
    access_host: &str,
    api_base_url: &str,
    force_overwrite: bool,
) -> Result<Option<String>, ExitError> {
    let p = paths.etc_xp_env();
    let existing = fs::read_to_string(&p).ok();
    let parsed = crate::ops::xp_env::parse_xp_env(existing);

    if let Some(v) = parsed.node_name.as_deref()
        && v != node_name
        && !force_overwrite
    {
        return Err(ExitError::new(
            2,
            "node_meta_mismatch: existing XP_NODE_NAME differs (use --overwrite-existing to replace)",
        ));
    }
    if let Some(v) = parsed.access_host.as_deref()
        && v != access_host
        && !force_overwrite
    {
        return Err(ExitError::new(
            2,
            "node_meta_mismatch: existing XP_ACCESS_HOST differs (use --overwrite-existing to replace)",
        ));
    }
    if let Some(v) = parsed.api_base_url.as_deref()
        && v != api_base_url
        && !force_overwrite
    {
        return Err(ExitError::new(
            2,
            "node_meta_mismatch: existing XP_API_BASE_URL differs (use --overwrite-existing to replace)",
        ));
    }

    if let Some(raw_hash) = parsed.admin_token_hash.as_deref() {
        if parse_admin_token_hash(raw_hash).is_none() {
            return Err(ExitError::new(
                2,
                "invalid_input: XP_ADMIN_TOKEN_HASH is present but invalid in /etc/xp/xp.env",
            ));
        }
        crate::ops::xp_env::write_xp_env(
            paths,
            mode,
            parsed.retained_lines,
            parsed.flags,
            crate::ops::xp_env::XpEnvWriteValues {
                admin_token_hash: raw_hash,
                node_name,
                access_host,
                api_base_url,
            },
        )?;
        return Ok(None);
    }

    if let Some(token) = parsed.admin_token_plain.as_deref() {
        let hash = hash_admin_token_argon2id(token)
            .map_err(|e| ExitError::new(2, format!("invalid_input: admin token hash: {e}")))?;
        crate::ops::xp_env::write_xp_env(
            paths,
            mode,
            parsed.retained_lines,
            parsed.flags,
            crate::ops::xp_env::XpEnvWriteValues {
                admin_token_hash: hash.as_str(),
                node_name,
                access_host,
                api_base_url,
            },
        )?;
        return Ok(None);
    }

    let token = generate_admin_token();
    let hash = hash_admin_token_argon2id(&token)
        .map_err(|e| ExitError::new(2, format!("invalid_input: admin token hash: {e}")))?;
    crate::ops::xp_env::write_xp_env(
        paths,
        mode,
        parsed.retained_lines,
        parsed.flags,
        crate::ops::xp_env::XpEnvWriteValues {
            admin_token_hash: hash.as_str(),
            node_name,
            access_host,
            api_base_url,
        },
    )?;
    Ok(Some(token))
}

fn ensure_xp_env_admin_token_hash_join(
    paths: &Paths,
    mode: Mode,
    expected_hash: &str,
    node_name: &str,
    access_host: &str,
    api_base_url: &str,
    force_overwrite: bool,
) -> Result<(), ExitError> {
    if parse_admin_token_hash(expected_hash).is_none() {
        return Err(ExitError::new(2, "invalid_args: expected hash is invalid"));
    }

    let p = paths.etc_xp_env();
    let existing = fs::read_to_string(&p).ok();
    let parsed = crate::ops::xp_env::parse_xp_env(existing);

    if let Some(v) = parsed.node_name.as_deref()
        && v != node_name
        && !force_overwrite
    {
        return Err(ExitError::new(
            2,
            "node_meta_mismatch: existing XP_NODE_NAME differs (use --overwrite-existing to replace)",
        ));
    }
    if let Some(v) = parsed.access_host.as_deref()
        && v != access_host
        && !force_overwrite
    {
        return Err(ExitError::new(
            2,
            "node_meta_mismatch: existing XP_ACCESS_HOST differs (use --overwrite-existing to replace)",
        ));
    }
    if let Some(v) = parsed.api_base_url.as_deref()
        && v != api_base_url
        && !force_overwrite
    {
        return Err(ExitError::new(
            2,
            "node_meta_mismatch: existing XP_API_BASE_URL differs (use --overwrite-existing to replace)",
        ));
    }

    if let Some(raw_hash) = parsed.admin_token_hash.as_deref() {
        if parse_admin_token_hash(raw_hash).is_none() {
            return Err(ExitError::new(
                2,
                "invalid_input: XP_ADMIN_TOKEN_HASH is present but invalid in /etc/xp/xp.env",
            ));
        }
        if raw_hash != expected_hash && !force_overwrite {
            return Err(ExitError::new(
                2,
                "admin_token_mismatch: existing XP_ADMIN_TOKEN_HASH differs (use --overwrite-existing to replace)",
            ));
        }
        crate::ops::xp_env::write_xp_env(
            paths,
            mode,
            parsed.retained_lines,
            parsed.flags,
            crate::ops::xp_env::XpEnvWriteValues {
                admin_token_hash: expected_hash,
                node_name,
                access_host,
                api_base_url,
            },
        )?;
        return Ok(());
    }

    if let Some(token) = parsed.admin_token_plain.as_deref() {
        let Some(expected) = parse_admin_token_hash(expected_hash) else {
            return Err(ExitError::new(2, "invalid_args: expected hash is invalid"));
        };
        if !verify_admin_token(token, &expected) && !force_overwrite {
            return Err(ExitError::new(
                2,
                "admin_token_mismatch: existing XP_ADMIN_TOKEN does not match cluster token (use --overwrite-existing to replace)",
            ));
        }
        crate::ops::xp_env::write_xp_env(
            paths,
            mode,
            parsed.retained_lines,
            parsed.flags,
            crate::ops::xp_env::XpEnvWriteValues {
                admin_token_hash: expected_hash,
                node_name,
                access_host,
                api_base_url,
            },
        )?;
        return Ok(());
    }

    crate::ops::xp_env::write_xp_env(
        paths,
        mode,
        parsed.retained_lines,
        parsed.flags,
        crate::ops::xp_env::XpEnvWriteValues {
            admin_token_hash: expected_hash,
            node_name,
            access_host,
            api_base_url,
        },
    )?;
    Ok(())
}

fn read_cluster_admin_token_hash(paths: &Paths, data_dir: &Path) -> Result<String, ExitError> {
    let abs_data_dir = paths.map_abs(data_dir);
    let cluster_paths = crate::cluster_metadata::ClusterPaths::new(&abs_data_dir);
    let raw = fs::read_to_string(&cluster_paths.admin_token_hash).map_err(|_| {
        ExitError::new(
            2,
            "admin_token_missing: cluster admin token hash not found (did xp join succeed?)",
        )
    })?;
    let hash = raw.trim();
    if parse_admin_token_hash(hash).is_none() {
        return Err(ExitError::new(
            2,
            "admin_token_invalid: cluster admin token hash is invalid",
        ));
    }
    Ok(hash.to_string())
}

fn generate_admin_token() -> String {
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const VALID_ADMIN_TOKEN_HASH: &str = "$argon2id$v=19$m=65536,t=3,p=1$TqOws+M/ypxKCmnVcbWAdg$VlLbEUvXvoESmlktijJp9QYD/jJklIIljA1vuce9P+k";

    fn read_env(paths: &Paths) -> String {
        fs::read_to_string(paths.etc_xp_env()).unwrap()
    }

    #[test]
    fn ensure_xp_env_admin_token_hash_keeps_xray_defaults_on_second_run() {
        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());

        fs::create_dir_all(paths.etc_xp_dir()).unwrap();
        fs::write(
            paths.etc_xp_env(),
            format!("XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\n"),
        )
        .unwrap();

        ensure_xp_env_admin_token_hash_bootstrap(
            &paths,
            Mode::Real,
            "node-1",
            "example.com",
            "https://example.com",
            false,
        )
        .unwrap();
        ensure_xp_env_admin_token_hash_bootstrap(
            &paths,
            Mode::Real,
            "node-1",
            "example.com",
            "https://example.com",
            false,
        )
        .unwrap();

        let env = read_env(&paths);
        assert!(env.contains(VALID_ADMIN_TOKEN_HASH));
        assert!(env.contains("XP_DATA_DIR="));
        assert!(env.contains("XP_XRAY_API_ADDR="));
        assert!(env.contains("XP_XRAY_HEALTH_INTERVAL_SECS="));
        assert!(env.contains("XP_XRAY_HEALTH_FAILS_BEFORE_DOWN="));
        assert!(env.contains("XP_XRAY_RESTART_MODE="));
        assert!(env.contains("XP_XRAY_RESTART_COOLDOWN_SECS="));
        assert!(env.contains("XP_XRAY_RESTART_TIMEOUT_SECS="));
        assert!(env.contains("XP_XRAY_SYSTEMD_UNIT="));
        assert!(env.contains("XP_XRAY_OPENRC_SERVICE="));
    }

    #[test]
    fn ensure_xp_env_admin_token_hash_preserves_user_xray_overrides() {
        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());

        fs::create_dir_all(paths.etc_xp_dir()).unwrap();
        fs::write(
            paths.etc_xp_env(),
            format!(
                "XP_ADMIN_TOKEN_HASH={VALID_ADMIN_TOKEN_HASH}\n\
XP_DATA_DIR=/custom/data\n\
XP_XRAY_API_ADDR=127.0.0.1:12345\n\
XP_XRAY_RESTART_MODE=systemd\n\
XP_XRAY_SYSTEMD_UNIT=custom-xray.service\n\
XP_XRAY_OPENRC_SERVICE=custom-xray\n\
XP_XRAY_CUSTOM=keep-me\n",
            ),
        )
        .unwrap();

        ensure_xp_env_admin_token_hash_bootstrap(
            &paths,
            Mode::Real,
            "node-1",
            "example.com",
            "https://example.com",
            false,
        )
        .unwrap();

        let env = read_env(&paths);
        assert!(env.contains("XP_DATA_DIR=/custom/data"));
        assert!(env.contains("XP_XRAY_API_ADDR=127.0.0.1:12345"));
        assert!(env.contains("XP_XRAY_RESTART_MODE=systemd"));
        assert!(env.contains("XP_XRAY_SYSTEMD_UNIT=custom-xray.service"));
        assert!(env.contains("XP_XRAY_OPENRC_SERVICE=custom-xray"));
        assert!(env.contains("XP_XRAY_CUSTOM=keep-me"));
    }

    #[tokio::test]
    async fn build_plan_cloudflare_token_missing_error_is_actionable() {
        let _lock = crate::ops::util::ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("CLOUDFLARE_API_TOKEN") };

        let tmp = tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());

        let xp_bin = tmp.path().join("xp");
        fs::write(&xp_bin, b"dummy").unwrap();

        let args = DeployArgs {
            xp_bin: Some(xp_bin),
            node_name: "node-1".to_string(),
            access_host: "node-1.example.net".to_string(),
            cloudflare_toggle: crate::ops::cli::CloudflareToggle {
                cloudflare: true,
                no_cloudflare: false,
            },
            account_id: Some("acc".to_string()),
            zone_id: Some("zone".to_string()),
            hostname: Some("node-1.example.com".to_string()),
            tunnel_name: None,
            origin_url: None,
            join_token: None,
            join_token_stdin: false,
            join_token_stdin_value: None,
            cloudflare_token: None,
            cloudflare_token_stdin: false,
            cloudflare_token_stdin_value: None,
            api_base_url: None,
            xray_version: "latest".to_string(),
            enable_services_toggle: crate::ops::cli::EnableServicesToggle {
                enable_services: false,
                no_enable_services: true,
            },
            yes: false,
            overwrite_existing: false,
            non_interactive: true,
            dry_run: true,
        };

        let plan = build_plan(&paths, &args).await.unwrap();
        assert!(
            plan.errors
                .iter()
                .any(|e| e.contains("--cloudflare-token") && e.contains("CLOUDFLARE_API_TOKEN")),
            "expected actionable token missing error, got: {:?}",
            plan.errors
        );
        assert!(
            !plan.errors.iter().any(|e| e.contains("token_missing")),
            "should not emit raw token_missing error string: {:?}",
            plan.errors
        );
    }
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
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(ExitError::new(
            2,
            "invalid_args: api-base-url must be an origin (no path/query)",
        ));
    }
    Ok(())
}
