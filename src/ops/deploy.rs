use crate::ops::cli::{
    DeployArgs, ExitError, InitArgs, InitSystemArg, InstallArgs, InstallOnly, XpBootstrapArgs,
    XpInstallArgs,
};
use crate::ops::cloudflare::{self, DnsRecordInfo, TunnelInfo, ZoneLookup};
use crate::ops::init;
use crate::ops::install;
use crate::ops::paths::Paths;
use crate::ops::platform::{InitSystem, detect_distro, detect_init_system};
use crate::ops::util::{Mode, chmod, ensure_dir, is_test_root, write_string_if_changed};
use crate::ops::xp;
use dialoguer::Confirm;
use dialoguer::Select;
use nanoid::nanoid;
use rand::RngCore;
use std::fs;
use std::io::{self, IsTerminal, Write};
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
    xp_bin: PathBuf,
    node_name: String,
    access_host: String,
    api_base_url: String,
    api_base_url_source: ValueSource,
    xray_version: String,
    enable_services: bool,
    cloudflare_enabled: bool,
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
    let interactive = !args.non_interactive && io::stdin().is_terminal();

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
                let (_new_args, new_plan) =
                    force_overwrite_tunnel_conflict(&paths, args, plan, mode == Mode::DryRun)
                        .await?;
                plan = new_plan;
                if !plan.errors.is_empty() {
                    return Err(ExitError::new(2, "preflight_failed: fix errors above"));
                }
            } else if auto_yes {
                let (_new_args, new_plan) =
                    auto_resolve_tunnel_conflict(&paths, args, plan, mode == Mode::DryRun).await?;
                plan = new_plan;
                if !plan.errors.is_empty() {
                    return Err(ExitError::new(2, "preflight_failed: fix errors above"));
                }
            } else if interactive {
                let (_new_args, new_plan) =
                    resolve_tunnel_conflict(&paths, args, plan, mode == Mode::DryRun).await?;
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
        eprintln!("  - write /etc/xp/xp.env (XP_ADMIN_TOKEN)");
        eprintln!("  - xp bootstrap (xp init)");
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
            xp_bin: plan.xp_bin.clone(),
            enable: false,
            dry_run: mode == Mode::DryRun,
        },
    )
    .await?;

    ensure_xp_env_admin_token(&paths, mode)?;

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

    if let Some(cf) = plan.cloudflare.clone() {
        cloudflare::cmd_cloudflare_provision(
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

    Ok(())
}

async fn build_plan(paths: &Paths, args: &DeployArgs) -> Result<DeployPlan, ExitError> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    if !args.xp_bin.exists() {
        errors.push("xp-bin does not exist".to_string());
    }

    if args.access_host.trim().is_empty() {
        warnings.push("access_host is empty".to_string());
    }

    let cloudflare_enabled = args.cloudflare_toggle.enabled();
    let mut api_base_url_source = ValueSource::Provided;
    let mut cloudflare_plan = None;

    let api_base_url = if cloudflare_enabled {
        let account_id = match args.account_id.clone() {
            Some(v) => v,
            None => {
                errors.push("missing --account-id with --cloudflare".to_string());
                String::new()
            }
        };

        let token = match cloudflare::load_cloudflare_token_for_ops(paths) {
            Ok(v) => Some(v),
            Err(e) => {
                errors.push(format!("cloudflare token error: {}", e.message));
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
        xp_bin: args.xp_bin.clone(),
        node_name: args.node_name.clone(),
        access_host: args.access_host.clone(),
        api_base_url,
        api_base_url_source,
        xray_version: args.xray_version.clone(),
        enable_services: args.enable_services_toggle.enabled(),
        cloudflare_enabled,
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

    line("xp_bin", plan.xp_bin.display().to_string());
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

fn ensure_xp_env_admin_token(paths: &Paths, mode: Mode) -> Result<(), ExitError> {
    let p = paths.etc_xp_env();
    if mode == Mode::DryRun {
        eprintln!("would ensure: {}", p.display());
        return Ok(());
    }

    ensure_dir(&paths.etc_xp_dir())
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;

    let existing = fs::read_to_string(&p).ok();
    let mut retained: Vec<String> = Vec::new();
    let mut admin_token: Option<String> = None;
    let mut has_data_dir = false;
    let mut has_xray_addr = false;
    if let Some(s) = existing {
        for line in s.lines() {
            if line.starts_with("XP_ADMIN_TOKEN=") {
                if line.len() > "XP_ADMIN_TOKEN=".len() {
                    admin_token = Some(line.trim().to_string());
                }
                continue;
            }
            if line.starts_with("XP_DATA_DIR=") {
                has_data_dir = true;
                continue;
            }
            if line.starts_with("XP_XRAY_API_ADDR=") {
                has_xray_addr = true;
                continue;
            }
            retained.push(line.to_string());
        }
    }

    let token_line = admin_token.unwrap_or_else(|| {
        let token = generate_admin_token();
        format!("XP_ADMIN_TOKEN={token}")
    });

    let mut lines = retained;
    lines.push(token_line);
    if !has_data_dir {
        lines.push("XP_DATA_DIR=/var/lib/xp/data".to_string());
    }
    if !has_xray_addr {
        lines.push("XP_XRAY_API_ADDR=127.0.0.1:10085".to_string());
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
