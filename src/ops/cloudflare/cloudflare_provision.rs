use super::*;
use crate::ops::cloudflare_config::{
    edit_local_config_for_hostname, ingress_hostnames, merge_remote_tunnel_config,
    remote_config_payload, remove_remote_hostname_rules,
};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProvisionRuntime {
    ManagedService,
    Container,
}

#[derive(Debug, Clone, Copy)]
struct ServiceState {
    enabled: bool,
    running: bool,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct FileMetadata {
    mode: u32,
    uid: u32,
    gid: u32,
}

pub(super) async fn run(
    paths: Paths,
    args: CloudflareProvisionArgs,
    token: String,
    runtime: ProvisionRuntime,
) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    let distro = detect_distro(&paths).map_err(|e| ExitError::new(2, e))?;
    let init_system = detect_init_system(distro, None);
    let cloudflared_binary = cloudflared_binary_path(&paths, distro);

    ensure_cloudflared_present(&paths, distro, mode).await?;

    let api_base = std::env::var("CLOUDFLARE_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.cloudflare.com".to_string());
    let client = CloudflareClient::new(api_base, token);

    let settings_before = load_settings_or_default(&paths)?;
    let mut settings = settings_before.clone();
    settings.enabled = args.enabled();
    settings.install_mode = match runtime {
        ProvisionRuntime::ManagedService => "external".to_string(),
        ProvisionRuntime::Container => "container".to_string(),
    };
    settings.account_id = args.account_id.clone();
    settings.zone_id = args.zone_id.clone();
    settings.hostname = args.hostname.clone();
    settings.origin_url = args.origin_url.clone();
    if let Some(id) = args.dns_record_id_override.clone() {
        settings.dns_record_id = Some(id);
    }
    let tunnel_name = args
        .tunnel_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "xp".to_string());

    let requested_tunnel_id = args.tunnel_id_override.clone().or_else(|| {
        (!args.migrate_existing_tunnel)
            .then(|| settings_before.tunnel_id.clone())
            .flatten()
    });
    let migrating_from = settings_before
        .tunnel_id
        .as_deref()
        .filter(|existing| requested_tunnel_id.as_deref() != Some(*existing));
    let old_preflight = if let Some(old_tunnel_id) = migrating_from {
        if settings_before.hostname != args.hostname || settings_before.zone_id != args.zone_id {
            return Err(ExitError::new(
                2,
                "migration_preflight_failed: settings hostname/zone must match migration target",
            ));
        }
        verify_tunnel_credentials(&paths, old_tunnel_id, "migration_preflight_failed")?;
        let old = client
            .get_tunnel_config(&args.account_id, old_tunnel_id)
            .await
            .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))?;
        let hostnames = ingress_hostnames(&old)
            .map_err(|e| ExitError::new(4, format!("migration_preflight_failed: {e}")))?;
        if !migration_owns_only_hostname(&hostnames, &args.hostname) {
            return Err(ExitError::new(
                2,
                "migration_preflight_failed: shared Tunnel requires another connector",
            ));
        }
        let dns = client
            .list_dns_records(&args.zone_id, &args.hostname)
            .await
            .map_err(|e| ExitError::new(5, format!("dns_error: {e}")))?;
        select_dns_record(
            &dns,
            settings.dns_record_id.as_deref(),
            &args.hostname,
            old_tunnel_id,
            Some(old_tunnel_id),
        )?;
        Some((old, dns))
    } else {
        None
    };
    let (tunnel_id, created) = if let Some(id) = requested_tunnel_id {
        (id, None)
    } else if mode == Mode::DryRun {
        if old_preflight.is_none() {
            eprintln!("dry-run: no existing tunnel id; changes are suppressed");
            return Ok(());
        }
        let dns = match old_preflight.as_ref() {
            Some((_, dns)) => dns.clone(),
            None => unreachable!("migration preflight is required before target creation"),
        };
        select_dns_record(
            &dns,
            settings.dns_record_id.as_deref(),
            &args.hostname,
            "new-tunnel",
            migrating_from,
        )?;
        eprintln!("dry-run: GET preflight completed for a new Tunnel");
        eprintln!(
            "dry-run: would {} the target CNAME only",
            if dns.is_empty() { "create" } else { "patch" }
        );
        eprintln!("dry-run: would create a Tunnel and replace XP ingress rules only");
        eprintln!(
            "dry-run: no POST/PUT/PATCH/DELETE, file write, or service restart was performed"
        );
        return Ok(());
    } else {
        let created = client
            .create_tunnel(&args.account_id, &tunnel_name)
            .await
            .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))?;
        let tunnel_id = created.id.clone();
        settings.tunnel_id = Some(tunnel_id.clone());
        (tunnel_id, Some(created))
    };
    let old_remote_before = old_preflight.as_ref().map(|(remote, _)| remote.clone());
    let old_remote_after = old_remote_before
        .as_ref()
        .map(|config| remove_remote_hostname_rules(config, &args.hostname))
        .transpose()
        .map_err(|e| ExitError::new(4, format!("migration_preflight_failed: {e}")))?;

    let preflight: Result<_, ExitError> = async {
        let remote_before = client
            .get_tunnel_config(&args.account_id, &tunnel_id)
            .await
            .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))?;
        let remote_after =
            merge_remote_tunnel_config(&remote_before, &args.hostname, &args.origin_url)
                .map_err(|e| ExitError::new(4, format!("cloudflare_config_error: {e}")))?;
        let dns_before = match old_preflight.as_ref() {
            Some((_, dns)) => dns.clone(),
            None => client
                .list_dns_records(&args.zone_id, &args.hostname)
                .await
                .map_err(|e| ExitError::new(5, format!("dns_error: {e}")))?,
        };
        let dns_target = select_dns_record(
            &dns_before,
            settings.dns_record_id.as_deref(),
            &args.hostname,
            &tunnel_id,
            migrating_from,
        )?;
        Ok((remote_before, remote_after, dns_before, dns_target))
    }
    .await;
    let (remote_before, remote_after, dns_before, dns_target) = match preflight {
        Ok(preflight) => preflight,
        Err(error) => {
            return Err(cleanup_created_tunnel_after_preflight(
                &client,
                &args.account_id,
                &tunnel_id,
                created.is_some(),
                error,
            )
            .await);
        }
    };

    if mode == Mode::DryRun {
        eprintln!("dry-run: GET preflight completed for tunnel {tunnel_id}");
        eprintln!(
            "dry-run: would replace XP ingress rules for {} only",
            args.hostname
        );
        eprintln!(
            "dry-run: would {} the target CNAME only",
            if dns_target.is_some() {
                "patch"
            } else {
                "create"
            }
        );
        eprintln!(
            "dry-run: no POST/PUT/PATCH/DELETE, file write, or service restart was performed"
        );
        return Ok(());
    }

    let cred_path_abs = format!("/etc/cloudflared/{tunnel_id}.json");
    let cred_path = paths
        .etc_cloudflared_dir()
        .join(format!("{tunnel_id}.json"));
    if created.is_none() && !cred_path.exists() {
        return Err(ExitError::new(
            6,
            format!(
                "filesystem_error: missing credentials file {}",
                cred_path.display()
            ),
        ));
    }
    if created.is_none() {
        verify_tunnel_credentials(&paths, &tunnel_id, "filesystem_error")?;
    }
    let config_path = paths.etc_cloudflared_config();
    let settings_path = paths.etc_xp_ops_cloudflare_settings();
    let local_before = read_optional_file(&config_path)?;
    let settings_before_bytes = read_optional_file(&settings_path)?;
    let config_metadata_before = capture_file_metadata(&config_path)?;
    let credential_metadata_before = capture_file_metadata(&cred_path)?;
    let service_file_path = cloudflared_service_file_path(&paths, init_system);
    let service_file_before = service_file_path
        .as_deref()
        .map(read_optional_file)
        .transpose()?
        .flatten();
    let service_file_metadata_before = service_file_path
        .as_deref()
        .map(capture_file_metadata)
        .transpose()?
        .flatten();
    let service_before = (args.enabled() && runtime == ProvisionRuntime::ManagedService)
        .then(|| capture_cloudflared_service_state(init_system, &paths))
        .transpose()?;
    let snapshot_dir = persist_rollback_snapshots(
        &paths,
        local_before.as_deref(),
        settings_before_bytes.as_deref(),
        &remote_before,
        old_remote_before.as_ref(),
        &dns_before,
        service_file_before.as_deref(),
    )?;
    let mut config_changed = false;
    let mut target_config_written = false;
    let mut old_config_written = false;
    let mut patched_dns: Option<DnsRecordInfo> = None;
    let mut created_dns_id: Option<String> = None;
    let mut service_change_attempted = false;

    let operation: Result<(), ExitError> = async {
        if runtime == ProvisionRuntime::ManagedService {
            service_change_attempted = true;
            ensure_cloudflared_service(&paths, distro, init_system, mode)?;
        }
        if let Some(created) = created.as_ref() {
            ensure_dir(&paths.etc_cloudflared_dir())
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            let cred_json = serde_json::to_string_pretty(&created.credentials_file)
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            write_string_if_changed(&cred_path, &(cred_json + "\n"))
                .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
            chmod(&cred_path, 0o600).ok();
        }
        config_changed = write_cloudflared_config(
            &paths,
            &tunnel_id,
            &cred_path_abs,
            &args.hostname,
            &args.origin_url,
            &cloudflared_binary,
        )?;
        ensure_cloudflared_file_ownership(&paths, &tunnel_id, mode, runtime)?;

        client
            .put_tunnel_config(
                &args.account_id,
                &tunnel_id,
                &remote_config_payload(remote_after.clone()),
            )
            .await
            .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))?;
        target_config_written = true;
        let dns_record_id = if let Some(record) = dns_target.as_ref() {
            client
                .patch_dns_record(&args.zone_id, &record.id, &args.hostname, &tunnel_id)
                .await
                .map_err(|e| ExitError::new(5, format!("dns_error: {e}")))?;
            patched_dns = Some(record.clone());
            record.id.clone()
        } else {
            let id = client
                .create_dns_record(&args.zone_id, &args.hostname, &tunnel_id)
                .await
                .map_err(|e| ExitError::new(5, format!("dns_error: {e}")))?;
            created_dns_id = Some(id.clone());
            id
        };

        if let (Some(old_tunnel_id), Some(old_config)) = (migrating_from, old_remote_after.as_ref())
        {
            client
                .put_tunnel_config(
                    &args.account_id,
                    old_tunnel_id,
                    &remote_config_payload(old_config.clone()),
                )
                .await
                .map_err(|e| ExitError::new(4, format!("cloudflare_api_error: {e}")))?;
            old_config_written = true;
        }

        settings.tunnel_id = Some(tunnel_id.clone());
        settings.dns_record_id = Some(dns_record_id);
        save_settings(&paths, &settings)?;

        if args.enabled() && runtime == ProvisionRuntime::ManagedService {
            enable_cloudflared_service(init_system, mode, &paths)?;
            if config_changed {
                restart_cloudflared_service(init_system, &paths)?;
                verify_cloudflared_service(init_system, &paths)?;
            }
        }
        Ok(())
    }
    .await;

    if let Err(error) = operation {
        let mut rollback_errors = Vec::new();
        if old_config_written
            && let (Some(old_tunnel_id), Some(old_config)) =
                (migrating_from, old_remote_before.as_ref())
            && let Err(rollback) = client
                .put_tunnel_config(
                    &args.account_id,
                    old_tunnel_id,
                    &remote_config_payload(old_config.clone()),
                )
                .await
        {
            rollback_errors.push(format!("restore old Tunnel {old_tunnel_id}: {rollback}"));
        }
        if let Some(id) = created_dns_id.as_deref()
            && let Err(rollback) = client.delete_dns_record(&args.zone_id, id).await
        {
            rollback_errors.push(format!("delete created DNS {id}: {rollback}"));
        }
        if let Some(record) = patched_dns.as_ref()
            && let Err(rollback) = client
                .patch_dns_content(&args.zone_id, &record.id, &record.content)
                .await
        {
            rollback_errors.push(format!("restore DNS {}: {rollback}", record.id));
        }
        if target_config_written
            && let Err(rollback) = client
                .put_tunnel_config(
                    &args.account_id,
                    &tunnel_id,
                    &remote_config_payload(remote_before.clone()),
                )
                .await
        {
            rollback_errors.push(format!("restore Tunnel {tunnel_id}: {rollback}"));
        }
        if let Err(rollback) = restore_optional_file(&config_path, local_before.as_deref()) {
            rollback_errors.push(format!("restore local config: {rollback}"));
        }
        if let Err(rollback) = restore_file_metadata(&config_path, config_metadata_before) {
            rollback_errors.push(format!("restore config metadata: {rollback}"));
        }
        if let Err(rollback) = restore_file_metadata(&cred_path, credential_metadata_before) {
            rollback_errors.push(format!("restore credential metadata: {rollback}"));
        }
        if let Some(service_file_path) = service_file_path.as_deref() {
            if let Err(rollback) =
                restore_optional_file(service_file_path, service_file_before.as_deref())
            {
                rollback_errors.push(format!("restore service file: {rollback}"));
            }
            if let Err(rollback) =
                restore_file_metadata(service_file_path, service_file_metadata_before)
            {
                rollback_errors.push(format!("restore service file metadata: {rollback}"));
            }
        }
        if service_file_path.is_some()
            && init_system == InitSystem::Systemd
            && let Err(rollback) = reload_systemd_units(&paths)
        {
            rollback_errors.push(format!(
                "reload restored systemd unit: {}",
                rollback.message
            ));
        }
        if let Err(rollback) =
            restore_optional_file(&settings_path, settings_before_bytes.as_deref())
        {
            rollback_errors.push(format!("restore settings: {rollback}"));
        }
        if created.is_some()
            && let Err(rollback) = fs::remove_file(&cred_path)
            && rollback.kind() != io::ErrorKind::NotFound
        {
            rollback_errors.push(format!("remove created credentials: {rollback}"));
        }
        if service_change_attempted
            && let Some(service_before) = service_before
            && let Err(rollback) =
                restore_cloudflared_service_state(init_system, &paths, service_before)
        {
            rollback_errors.push(format!(
                "restore cloudflared service state: {}",
                rollback.message
            ));
        }
        if created.is_some()
            && let Err(rollback) = client.delete_tunnel(&args.account_id, &tunnel_id).await
        {
            rollback_errors.push(format!("delete created Tunnel {tunnel_id}: {rollback}"));
        }
        if rollback_errors.is_empty() {
            let _ = fs::remove_dir_all(&snapshot_dir);
            return Err(ExitError::new(
                error.code,
                format!("{}; rollback completed", error.message),
            ));
        }
        return Err(ExitError::new(
            error.code,
            format!(
                "{}; rollback incomplete; snapshots retained at {}: {}",
                error.message,
                snapshot_dir.display(),
                rollback_errors.join("; ")
            ),
        ));
    }
    let _ = fs::remove_dir_all(&snapshot_dir);
    Ok(())
}

fn migration_owns_only_hostname(hostnames: &[String], hostname: &str) -> bool {
    !hostnames.is_empty() && hostnames.iter().all(|existing| existing == hostname)
}

fn restart_cloudflared_service(init_system: InitSystem, paths: &Paths) -> Result<(), ExitError> {
    if is_test_root(paths.root()) {
        return Ok(());
    }
    let status = match init_system {
        InitSystem::Systemd => Command::new("systemctl")
            .args(["restart", "cloudflared.service"])
            .status(),
        InitSystem::OpenRc => Command::new("rc-service")
            .args(["cloudflared", "restart"])
            .status(),
        InitSystem::None => return Ok(()),
    }
    .map_err(|error| ExitError::new(6, format!("service_error: cloudflared restart: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(ExitError::new(
            6,
            "service_error: cloudflared restart failed",
        ))
    }
}

fn verify_cloudflared_service(init_system: InitSystem, paths: &Paths) -> Result<(), ExitError> {
    if is_test_root(paths.root()) {
        return Ok(());
    }
    let status = match init_system {
        InitSystem::Systemd => Command::new("systemctl")
            .args(["is-active", "--quiet", "cloudflared.service"])
            .status(),
        InitSystem::OpenRc => Command::new("rc-service")
            .args(["cloudflared", "status"])
            .status(),
        InitSystem::None => return Ok(()),
    }
    .map_err(|error| {
        ExitError::new(
            6,
            format!("service_error: cloudflared health check: {error}"),
        )
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(ExitError::new(
            6,
            "service_error: cloudflared is not healthy after restart",
        ))
    }
}

fn capture_cloudflared_service_state(
    init_system: InitSystem,
    paths: &Paths,
) -> Result<ServiceState, ExitError> {
    if is_test_root(paths.root()) || init_system == InitSystem::None {
        return Ok(ServiceState {
            enabled: false,
            running: false,
        });
    }
    let (enabled, running) = match init_system {
        InitSystem::Systemd => (
            service_command_status(
                "systemctl",
                &["is-enabled", "--quiet", "cloudflared.service"],
            )?
            .success(),
            service_command_status(
                "systemctl",
                &["is-active", "--quiet", "cloudflared.service"],
            )?
            .success(),
        ),
        InitSystem::OpenRc => {
            let output = Command::new("rc-update")
                .args(["show", "default"])
                .output()
                .map_err(|error| {
                    ExitError::new(6, format!("service_error: rc-update show: {error}"))
                })?;
            let enabled = String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.split_whitespace().next() == Some("cloudflared"));
            let running =
                service_command_status("rc-service", &["cloudflared", "status"])?.success();
            (enabled, running)
        }
        InitSystem::None => unreachable!(),
    };
    Ok(ServiceState { enabled, running })
}

fn restore_cloudflared_service_state(
    init_system: InitSystem,
    paths: &Paths,
    state: ServiceState,
) -> Result<(), ExitError> {
    if is_test_root(paths.root()) || init_system == InitSystem::None {
        return Ok(());
    }
    let (enable_command, disable_command, start_command, stop_command) = match init_system {
        InitSystem::Systemd => (
            ("systemctl", &["enable", "cloudflared.service"][..]),
            ("systemctl", &["disable", "cloudflared.service"][..]),
            ("systemctl", &["start", "cloudflared.service"][..]),
            ("systemctl", &["stop", "cloudflared.service"][..]),
        ),
        InitSystem::OpenRc => (
            ("rc-update", &["add", "cloudflared", "default"][..]),
            ("rc-update", &["del", "cloudflared", "default"][..]),
            ("rc-service", &["cloudflared", "start"][..]),
            ("rc-service", &["cloudflared", "stop"][..]),
        ),
        InitSystem::None => unreachable!(),
    };
    let command = if state.enabled {
        enable_command
    } else {
        disable_command
    };
    ensure_service_command(command.0, command.1)?;
    let command = if state.running {
        start_command
    } else {
        stop_command
    };
    ensure_service_command(command.0, command.1)
}

fn reload_systemd_units(paths: &Paths) -> Result<(), ExitError> {
    if is_test_root(paths.root()) {
        return Ok(());
    }
    ensure_service_command("systemctl", &["daemon-reload"])
}

fn cloudflared_service_file_path(
    paths: &Paths,
    init_system: InitSystem,
) -> Option<std::path::PathBuf> {
    match init_system {
        InitSystem::Systemd => Some(paths.systemd_unit_dir().join("cloudflared.service")),
        InitSystem::OpenRc => Some(paths.openrc_initd_dir().join("cloudflared")),
        InitSystem::None => None,
    }
}

fn service_command_status(
    program: &str,
    args: &[&str],
) -> Result<std::process::ExitStatus, ExitError> {
    Command::new(program)
        .args(args)
        .status()
        .map_err(|error| ExitError::new(6, format!("service_error: {program}: {error}")))
}

fn ensure_service_command(program: &str, args: &[&str]) -> Result<(), ExitError> {
    if service_command_status(program, args)?.success() {
        Ok(())
    } else {
        Err(ExitError::new(
            6,
            format!("service_error: {program} {:?}", args),
        ))
    }
}

pub(super) fn select_dns_record(
    records: &[DnsRecordInfo],
    configured_id: Option<&str>,
    hostname: &str,
    tunnel_id: &str,
    previous_tunnel_id: Option<&str>,
) -> Result<Option<DnsRecordInfo>, ExitError> {
    if let Some(id) = configured_id {
        let record = records
            .iter()
            .find(|record| record.id == id)
            .ok_or_else(|| {
                ExitError::new(
                    5,
                    "dns_error: configured DNS record is missing or outside the hostname",
                )
            })?;
        if !is_owned_tunnel_record(record, hostname, tunnel_id)
            && !previous_tunnel_id.is_some_and(|old_tunnel_id| {
                is_owned_tunnel_record(record, hostname, old_tunnel_id)
            })
        {
            return Err(ExitError::new(
                5,
                "dns_error: configured DNS record is not the owned Tunnel CNAME",
            ));
        }
        return Ok(Some(record.clone()));
    }
    match records {
        [] => Ok(None),
        [record]
            if is_owned_tunnel_record(record, hostname, tunnel_id)
                || previous_tunnel_id.is_some_and(|old_tunnel_id| {
                    is_owned_tunnel_record(record, hostname, old_tunnel_id)
                }) =>
        {
            Ok(Some(record.clone()))
        }
        _ => Err(ExitError::new(
            5,
            "dns_error: hostname has an unmanaged or ambiguous DNS record; refusing to modify it",
        )),
    }
}

fn is_owned_tunnel_record(record: &DnsRecordInfo, hostname: &str, tunnel_id: &str) -> bool {
    record.record_type.eq_ignore_ascii_case("CNAME")
        && record.name.eq_ignore_ascii_case(hostname)
        && record
            .content
            .trim_end_matches('.')
            .eq_ignore_ascii_case(&format!("{tunnel_id}.cfargotunnel.com"))
}

fn write_cloudflared_config(
    paths: &Paths,
    tunnel_id: &str,
    cred_abs: &str,
    hostname: &str,
    origin_url: &str,
    cloudflared_binary: &Path,
) -> Result<bool, ExitError> {
    let config_path = paths.etc_cloudflared_config();
    let original = match fs::read(&config_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(ExitError::new(6, format!("filesystem_error: {error}"))),
    };
    let yml = edit_local_config_for_hostname(
        original.as_deref(),
        tunnel_id,
        cred_abs,
        hostname,
        origin_url,
    )
    .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?;
    if original.as_deref() == Some(yml.as_slice()) {
        return Ok(false);
    }
    validate_cloudflared_config(paths, cloudflared_binary, &yml)?;
    write_bytes_if_changed(&config_path, &yml)
        .map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))?;
    chmod(&config_path, 0o640).ok();
    Ok(true)
}

fn verify_tunnel_credentials(
    paths: &Paths,
    tunnel_id: &str,
    error_prefix: &str,
) -> Result<(), ExitError> {
    let path = paths
        .etc_cloudflared_dir()
        .join(format!("{tunnel_id}.json"));
    let raw = fs::read(&path).map_err(|error| {
        ExitError::new(
            6,
            format!("{error_prefix}: credentials {}: {error}", path.display()),
        )
    })?;
    let credentials: serde_json::Value = serde_json::from_slice(&raw).map_err(|error| {
        ExitError::new(
            6,
            format!("{error_prefix}: credentials {}: {error}", path.display()),
        )
    })?;
    if credentials
        .get("TunnelID")
        .and_then(serde_json::Value::as_str)
        == Some(tunnel_id)
    {
        Ok(())
    } else {
        Err(ExitError::new(
            6,
            format!(
                "{error_prefix}: credentials {} do not belong to Tunnel {tunnel_id}",
                path.display()
            ),
        ))
    }
}

fn validate_cloudflared_config(
    paths: &Paths,
    cloudflared_binary: &Path,
    config: &[u8],
) -> Result<(), ExitError> {
    if is_test_root(paths.root()) {
        return Ok(());
    }
    let config_path = paths.etc_cloudflared_config();
    let candidate_path = crate::ops::util::tmp_path_next_to(&config_path);
    fs::write(&candidate_path, config)
        .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?;
    let status = Command::new(cloudflared_binary)
        .args([
            "--config",
            candidate_path.to_string_lossy().as_ref(),
            "tunnel",
            "ingress",
            "validate",
        ])
        .status()
        .map_err(|error| ExitError::new(6, format!("cloudflared_validate_error: {error}")));
    let _ = fs::remove_file(&candidate_path);
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(ExitError::new(
            6,
            "cloudflared_validate_error: ingress validation failed",
        )),
        Err(error) => Err(error),
    }
}

async fn cleanup_created_tunnel_after_preflight(
    client: &CloudflareClient,
    account_id: &str,
    tunnel_id: &str,
    created: bool,
    error: ExitError,
) -> ExitError {
    if !created {
        return error;
    }
    match client.delete_tunnel(account_id, tunnel_id).await {
        Ok(()) => ExitError::new(
            error.code,
            format!(
                "{}; deleted newly created Tunnel after preflight failure",
                error.message
            ),
        ),
        Err(rollback) => ExitError::new(
            error.code,
            format!(
                "{}; failed to delete newly created Tunnel {tunnel_id}: {rollback}",
                error.message
            ),
        ),
    }
}

fn load_settings_or_default(paths: &Paths) -> Result<Settings, ExitError> {
    let p = paths.etc_xp_ops_cloudflare_settings();
    let Ok(raw) = fs::read_to_string(&p) else {
        return Ok(Settings::default());
    };
    serde_json::from_str(&raw).map_err(|e| ExitError::new(6, format!("filesystem_error: {e}")))
}

fn read_optional_file(path: &Path) -> Result<Option<Vec<u8>>, ExitError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ExitError::new(6, format!("filesystem_error: {error}"))),
    }
}

#[cfg(unix)]
fn capture_file_metadata(path: &Path) -> Result<Option<FileMetadata>, ExitError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(FileMetadata {
            mode: metadata.permissions().mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
        })),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ExitError::new(6, format!("filesystem_error: {error}"))),
    }
}

#[cfg(not(unix))]
fn capture_file_metadata(_path: &Path) -> Result<Option<()>, ExitError> {
    Ok(None)
}

#[cfg(unix)]
fn restore_file_metadata(path: &Path, metadata: Option<FileMetadata>) -> Result<(), io::Error> {
    let Some(metadata) = metadata else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(metadata.mode))?;
    std::os::unix::fs::chown(path, Some(metadata.uid), Some(metadata.gid))
}

#[cfg(not(unix))]
fn restore_file_metadata(_path: &Path, _metadata: Option<()>) -> Result<(), io::Error> {
    Ok(())
}

fn restore_optional_file(path: &Path, original: Option<&[u8]>) -> Result<(), io::Error> {
    match original {
        Some(bytes) => {
            write_bytes_if_changed(path, bytes)?;
            Ok(())
        }
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        },
    }
}

fn persist_rollback_snapshots(
    paths: &Paths,
    local_config: Option<&[u8]>,
    settings: Option<&[u8]>,
    target_remote: &serde_json::Value,
    old_remote: Option<&serde_json::Value>,
    dns: &[DnsRecordInfo],
    service_file: Option<&[u8]>,
) -> Result<std::path::PathBuf, ExitError> {
    let directory = paths
        .etc_xp_ops_cloudflare_dir()
        .join(format!(".rollback-{}", std::process::id()));
    ensure_dir(&directory)
        .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?;
    if let Some(bytes) = local_config {
        fs::write(directory.join("config.yml"), bytes)
            .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?;
    }
    if let Some(bytes) = settings {
        fs::write(directory.join("settings.json"), bytes)
            .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?;
    }
    fs::write(
        directory.join("target-tunnel.json"),
        serde_json::to_vec_pretty(target_remote)
            .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?,
    )
    .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?;
    if let Some(value) = old_remote {
        fs::write(
            directory.join("old-tunnel.json"),
            serde_json::to_vec_pretty(value)
                .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?,
        )
        .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?;
    }
    if let Some(bytes) = service_file {
        fs::write(directory.join("service-definition"), bytes)
            .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?;
    }
    fs::write(
        directory.join("dns.json"),
        serde_json::to_vec_pretty(dns)
            .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?,
    )
    .map_err(|error| ExitError::new(6, format!("filesystem_error: {error}")))?;
    Ok(directory)
}

#[cfg(test)]
#[path = "cloudflare_provision_tests.rs"]
mod tests;
