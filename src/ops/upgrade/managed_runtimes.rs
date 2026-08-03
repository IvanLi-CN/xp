use super::*;
use crate::ops::init::{backfill_low_memory_runtime_defaults, write_static_xray_config};
use crate::ops::runtime_activation::{
    reload_systemd_units, restart_cloudflared_service, restart_xray_service,
};

#[derive(Debug)]
pub(super) struct RuntimeBinaryBackup {
    dest: PathBuf,
    backup: Option<PathBuf>,
}

#[derive(Debug)]
struct RuntimeDefaultsBackup {
    files: Vec<RuntimeDefaultFileBackup>,
}

#[derive(Debug)]
struct RuntimeDefaultFileBackup {
    path: PathBuf,
    contents: Option<Vec<u8>>,
    mode: Option<u32>,
    parent_existed: bool,
}

fn xray_asset_name(platform: Platform) -> &'static str {
    match platform {
        Platform::LinuxX86_64 => "xray-linux-x86_64",
        Platform::LinuxAarch64 => "xray-linux-aarch64",
    }
}

fn cloudflared_asset_name(platform: Platform) -> &'static str {
    match platform {
        Platform::LinuxX86_64 => "cloudflared-linux-x86_64",
        Platform::LinuxAarch64 => "cloudflared-linux-aarch64",
    }
}

pub(super) fn describe_dry_run(platform: Platform) {
    for asset in [xray_asset_name(platform), cloudflared_asset_name(platform)] {
        eprintln!("would install managed runtime asset when present: {asset}");
    }
}

pub(super) async fn upgrade_and_reconcile_managed_runtimes(
    paths: &Paths,
    release: &GitHubRelease,
    checksums: &HashMap<String, [u8; 32]>,
    platform: Platform,
    xp_backup: &Path,
    rollback_xp_on_failure: bool,
) -> Result<(), ExitError> {
    let backups = match upgrade_managed_runtime_binaries(paths, release, checksums, platform).await
    {
        Ok(backups) => backups,
        Err(err) => {
            return finish_runtime_failure(paths, xp_backup, err, rollback_xp_on_failure);
        }
    };
    let runtime_defaults = match snapshot_runtime_defaults(paths) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            let err = retain_original_error(
                err,
                rollback_runtime_binaries_and_services(
                    paths,
                    &backups,
                    &RuntimeDefaultsBackup { files: Vec::new() },
                ),
            );
            return finish_runtime_failure(paths, xp_backup, err, rollback_xp_on_failure);
        }
    };
    if let Err(err) = reconcile_static_xray_config_and_restart(paths) {
        let err = retain_original_error(
            err,
            rollback_runtime_binaries_and_services(paths, &backups, &runtime_defaults),
        );
        return finish_runtime_failure(paths, xp_backup, err, rollback_xp_on_failure);
    }
    Ok(())
}

fn finish_runtime_failure(
    paths: &Paths,
    xp_backup: &Path,
    error: ExitError,
    rollback_xp_on_failure: bool,
) -> Result<(), ExitError> {
    if rollback_xp_on_failure {
        rollback_xp_after_xray_failure(paths, xp_backup, error)
    } else {
        Err(error)
    }
}

fn retain_original_error(original: ExitError, rollback: Result<(), ExitError>) -> ExitError {
    match rollback {
        Ok(()) => original,
        Err(rollback) => ExitError::new(
            rollback.code,
            format!(
                "{}; runtime rollback failed: {}",
                original.message, rollback.message
            ),
        ),
    }
}

fn rollback_runtime_binaries_and_services(
    paths: &Paths,
    backups: &[RuntimeBinaryBackup],
    runtime_defaults: &RuntimeDefaultsBackup,
) -> Result<(), ExitError> {
    rollback_runtime_binaries(backups)?;
    restore_runtime_defaults(runtime_defaults)?;
    if !reload_systemd_units(paths) {
        return Err(ExitError::new(
            8,
            "rollback_failed: restored runtime defaults but systemd daemon-reload failed",
        ));
    }
    let xray_ok = restart_xray_service(
        paths,
        &read_xray_systemd_unit(paths),
        &read_xray_openrc_service(paths),
    );
    let cloudflared_ok = restart_cloudflared_service(paths);
    if xray_ok && cloudflared_ok {
        Ok(())
    } else {
        Err(ExitError::new(
            8,
            "rollback_failed: restored runtime binaries but service restart failed",
        ))
    }
}

fn snapshot_runtime_defaults(paths: &Paths) -> Result<RuntimeDefaultsBackup, ExitError> {
    let paths = [
        paths
            .systemd_unit_dir()
            .join("xray.service.d/20-xp-memory.conf"),
        paths
            .systemd_unit_dir()
            .join("cloudflared.service.d/20-xp-memory.conf"),
        paths.systemd_unit_dir().join("cloudflared.service"),
        paths.openrc_initd_dir().join("xray"),
        paths.openrc_initd_dir().join("cloudflared"),
    ];
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let parent_existed = path.parent().is_some_and(Path::exists);
        let (contents, mode) = match fs::metadata(&path) {
            Ok(metadata) => (
                Some(fs::read(&path).map_err(|error| {
                    ExitError::new(
                        8,
                        format!("rollback_failed: snapshot runtime defaults: {error}"),
                    )
                })?),
                Some(std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o777),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None),
            Err(error) => {
                return Err(ExitError::new(
                    8,
                    format!("rollback_failed: snapshot runtime defaults: {error}"),
                ));
            }
        };
        files.push(RuntimeDefaultFileBackup {
            path,
            contents,
            mode,
            parent_existed,
        });
    }
    Ok(RuntimeDefaultsBackup { files })
}

fn restore_runtime_defaults(snapshot: &RuntimeDefaultsBackup) -> Result<(), ExitError> {
    for file in &snapshot.files {
        match file.contents.as_deref() {
            Some(contents) => {
                let parent = file.path.parent().ok_or_else(|| {
                    ExitError::new(
                        8,
                        "rollback_failed: runtime default has no parent directory",
                    )
                })?;
                ensure_dir(parent).map_err(|error| {
                    ExitError::new(
                        8,
                        format!("rollback_failed: restore runtime defaults directory: {error}"),
                    )
                })?;
                fs::write(&file.path, contents).map_err(|error| {
                    ExitError::new(
                        8,
                        format!("rollback_failed: restore runtime defaults: {error}"),
                    )
                })?;
                if let Some(mode) = file.mode {
                    fs::set_permissions(
                        &file.path,
                        std::os::unix::fs::PermissionsExt::from_mode(mode),
                    )
                    .map_err(|error| {
                        ExitError::new(
                            8,
                            format!("rollback_failed: restore runtime default mode: {error}"),
                        )
                    })?;
                }
            }
            None => match fs::remove_file(&file.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(ExitError::new(
                        8,
                        format!("rollback_failed: remove created runtime defaults: {error}"),
                    ));
                }
            },
        }
        if !file.parent_existed
            && let Some(parent) = file.path.parent()
        {
            match fs::remove_dir(parent) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(error) => {
                    return Err(ExitError::new(
                        8,
                        format!("rollback_failed: remove runtime defaults directory: {error}"),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn reconcile_static_xray_config_and_restart(paths: &Paths) -> Result<(), ExitError> {
    backfill_low_memory_runtime_defaults(paths)?;
    if !reload_systemd_units(paths) {
        return Err(ExitError::new(
            7,
            "service_error: systemd daemon-reload failed",
        ));
    }
    let config_path = paths.etc_xray_config();
    let backup = backup_path(&config_path);
    let had_old = config_path.exists();
    let existing_config = if had_old {
        Some(
            fs::read_to_string(&config_path)
                .map_err(|e| ExitError::new(7, format!("service_error: read xray config: {e}")))?,
        )
    } else {
        None
    };
    if had_old {
        fs::copy(&config_path, &backup)
            .map_err(|e| ExitError::new(7, format!("service_error: backup xray config: {e}")))?;
    }
    if let Err(err) = write_static_xray_config(paths)
        .and_then(|_| preserve_control_plane_listeners(paths, existing_config.as_deref()))
    {
        if had_old {
            let _ = fs::copy(&backup, &config_path);
            let _ = fs::remove_file(&backup);
        }
        return Err(err);
    }

    if restart_xray_service(
        paths,
        &read_xray_systemd_unit(paths),
        &read_xray_openrc_service(paths),
    ) {
        if !restart_cloudflared_service(paths) {
            if had_old {
                fs::copy(&backup, &config_path).map_err(|e| {
                    ExitError::new(8, format!("rollback_failed: restore xray config: {e}"))
                })?;
            } else {
                let _ = fs::remove_file(&config_path);
            }
            let rollback_restarted = restart_xray_service(
                paths,
                &read_xray_systemd_unit(paths),
                &read_xray_openrc_service(paths),
            );
            let _ = fs::remove_file(&backup);
            return if rollback_restarted {
                Err(ExitError::new(
                    7,
                    "service_error: cloudflared restart failed; restored previous xray config",
                ))
            } else {
                Err(ExitError::new(
                    8,
                    concat!(
                        "rollback_failed: cloudflared restart failed; restored previous xray ",
                        "config; xray rollback restart failed",
                    ),
                ))
            };
        }
        if had_old {
            let _ = fs::remove_file(&backup);
        }
        return Ok(());
    }

    if !had_old {
        let _ = fs::remove_file(&config_path);
        return Err(ExitError::new(7, "service_error: xray restart failed"));
    }

    fs::copy(&backup, &config_path)
        .map_err(|e| ExitError::new(8, format!("rollback_failed: restore xray config: {e}")))?;
    let rollback_restarted = restart_xray_service(
        paths,
        &read_xray_systemd_unit(paths),
        &read_xray_openrc_service(paths),
    );
    let _ = fs::remove_file(&backup);

    Err(ExitError::new(
        7,
        if rollback_restarted {
            "service_error: xray restart failed; restored previous config"
        } else {
            "service_error: xray restart failed; restored previous config; rollback restart failed"
        },
    ))
}

pub(super) async fn upgrade_managed_runtime_binaries(
    paths: &Paths,
    release: &GitHubRelease,
    checksums: &HashMap<String, [u8; 32]>,
    platform: Platform,
) -> Result<Vec<RuntimeBinaryBackup>, ExitError> {
    let xray_asset = xray_asset_name(platform);
    let cloudflared_asset = cloudflared_asset_name(platform);
    let has_xray = find_asset_url(release, xray_asset).is_some();
    let has_cloudflared = find_asset_url(release, cloudflared_asset).is_some();
    if !has_xray && !has_cloudflared {
        return Ok(Vec::new());
    }
    if !has_xray || !has_cloudflared {
        return Err(ExitError::new(
            5,
            "download_failed: managed runtime assets must be published together",
        ));
    }

    let mut installed = Vec::new();
    for (asset, dest) in [
        (xray_asset, paths.usr_local_bin_xray()),
        (cloudflared_asset, managed_cloudflared_dest(paths)),
    ] {
        match install_release_binary(release, checksums, asset, &dest).await {
            Ok(backup) => installed.push(backup),
            Err(err) => {
                rollback_runtime_binaries(&installed)?;
                return Err(err);
            }
        }
    }
    Ok(installed)
}

fn managed_cloudflared_dest(paths: &Paths) -> PathBuf {
    if paths.openrc_initd_dir().join("cloudflared").exists() {
        return paths.usr_local_bin_cloudflared();
    }
    let usr_bin = paths.usr_bin_cloudflared();
    if paths
        .systemd_unit_dir()
        .join("cloudflared.service")
        .exists()
        && usr_bin.exists()
    {
        return usr_bin;
    }
    let usr_local = paths.usr_local_bin_cloudflared();
    if usr_local.exists() {
        usr_local
    } else {
        usr_bin
    }
}

async fn install_release_binary(
    release: &GitHubRelease,
    checksums: &HashMap<String, [u8; 32]>,
    asset_name: &str,
    dest: &Path,
) -> Result<RuntimeBinaryBackup, ExitError> {
    let asset_url = find_asset_url(release, asset_name)
        .ok_or_else(|| ExitError::new(5, format!("download_failed: missing asset {asset_name}")))?;
    let expected = checksums.get(asset_name).ok_or_else(|| {
        ExitError::new(
            6,
            format!("checksum_mismatch: missing {asset_name} in {CHECKSUMS_ASSET_NAME}"),
        )
    })?;
    if let Some(parent) = dest.parent() {
        ensure_dir(parent).map_err(|e| ExitError::new(7, format!("install_failed: {e}")))?;
    }
    let staged = tmp_path_next_to(dest);
    download_to_path(asset_url, &staged)
        .await
        .map_err(|e| ExitError::new(5, format!("download_failed: {e}")))?;
    if sha256_file(&staged)? != *expected {
        let _ = fs::remove_file(&staged);
        return Err(ExitError::new(6, "checksum_mismatch"));
    }
    chmod(&staged, 0o755).ok();

    let backup = if dest.exists() {
        let backup = backup_path(dest);
        fs::rename(dest, &backup).map_err(|e| ExitError::new(7, format!("install_failed: {e}")))?;
        Some(backup)
    } else {
        None
    };
    if let Err(err) = fs::rename(&staged, dest) {
        if let Some(backup) = backup.as_ref() {
            let _ = fs::rename(backup, dest);
        }
        let _ = fs::remove_file(&staged);
        return Err(ExitError::new(7, format!("install_failed: {err}")));
    }
    chmod(dest, 0o755).ok();
    Ok(RuntimeBinaryBackup {
        dest: dest.to_path_buf(),
        backup,
    })
}

pub(super) fn rollback_runtime_binaries(backups: &[RuntimeBinaryBackup]) -> Result<(), ExitError> {
    for installed in backups.iter().rev() {
        if installed.dest.exists() {
            let failed = installed
                .dest
                .with_extension(format!("failed.{}", now_unix_secs()));
            fs::rename(&installed.dest, &failed).map_err(|e| {
                ExitError::new(8, format!("rollback_failed: stash runtime binary: {e}"))
            })?;
        }
        if let Some(backup) = installed.backup.as_ref() {
            fs::rename(backup, &installed.dest).map_err(|e| {
                ExitError::new(8, format!("rollback_failed: restore runtime binary: {e}"))
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_rollback_restores_both_binaries() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        let bin = tmp.path().join("usr/local/bin");
        fs::create_dir_all(&bin).unwrap();
        let xray = bin.join("xray");
        let cloudflared = bin.join("cloudflared");
        let xray_backup = bin.join("xray.bak.test");
        let cloudflared_backup = bin.join("cloudflared.bak.test");
        fs::write(&xray, b"new-xray").unwrap();
        fs::write(&cloudflared, b"new-cloudflared").unwrap();
        fs::write(&xray_backup, b"old-xray").unwrap();
        fs::write(&cloudflared_backup, b"old-cloudflared").unwrap();

        rollback_runtime_binaries_and_services(
            &paths,
            &[
                RuntimeBinaryBackup {
                    dest: xray.clone(),
                    backup: Some(xray_backup),
                },
                RuntimeBinaryBackup {
                    dest: cloudflared.clone(),
                    backup: Some(cloudflared_backup),
                },
            ],
            &RuntimeDefaultsBackup { files: Vec::new() },
        )
        .unwrap();

        assert_eq!(fs::read(xray).unwrap(), b"old-xray");
        assert_eq!(fs::read(cloudflared).unwrap(), b"old-cloudflared");
    }

    #[test]
    fn runtime_rollback_failure_keeps_original_error() {
        let result = retain_original_error(
            ExitError::new(7, "service_error: cloudflared restart failed"),
            Err(ExitError::new(
                8,
                "rollback_failed: restored runtime binaries but service restart failed",
            )),
        );

        assert_eq!(result.code, 8);
        assert!(result.message.contains("cloudflared restart failed"));
        assert!(result.message.contains("runtime rollback failed"));
    }

    #[test]
    fn runtime_defaults_rollback_restores_pre_upgrade_files() {
        use crate::ops::init::backfill_low_memory_runtime_defaults;

        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        let systemd = paths.systemd_unit_dir();
        fs::create_dir_all(&systemd).unwrap();
        fs::write(systemd.join("xray.service"), "[Service]\n").unwrap();
        let cloudflared_unit = systemd.join("cloudflared.service");
        let original_unit = concat!(
            "[Service]\n",
            "ExecStart=/usr/bin/cloudflared --no-autoupdate ",
            "--config /etc/cloudflared/config.yml tunnel run\n",
        );
        fs::write(&cloudflared_unit, original_unit).unwrap();
        let cloudflared_drop_in = systemd.join("cloudflared.service.d/20-xp-memory.conf");
        fs::create_dir_all(cloudflared_drop_in.parent().unwrap()).unwrap();
        let original_drop_in = "[Service]\nEnvironment=GOMEMLIMIT=12MiB\nEnvironment=GOGC=50\n";
        fs::write(&cloudflared_drop_in, original_drop_in).unwrap();

        let openrc = paths.openrc_initd_dir();
        fs::create_dir_all(&openrc).unwrap();
        let cloudflared_openrc = openrc.join("cloudflared");
        let original_openrc = concat!(
            "command_user=\"cloudflared:cloudflared\"\n",
            "export GOMEMLIMIT=\"${GOMEMLIMIT:-12MiB}\"\n",
            "export GOGC=\"${GOGC:-50}\"\n",
        );
        fs::write(&cloudflared_openrc, original_openrc).unwrap();
        fs::set_permissions(
            &cloudflared_openrc,
            std::os::unix::fs::PermissionsExt::from_mode(0o640),
        )
        .unwrap();

        let snapshot = snapshot_runtime_defaults(&paths).unwrap();
        backfill_low_memory_runtime_defaults(&paths).unwrap();
        assert!(
            fs::read_to_string(&cloudflared_drop_in)
                .unwrap()
                .contains("GOMEMLIMIT=8MiB")
        );
        assert!(
            fs::read_to_string(&cloudflared_openrc)
                .unwrap()
                .contains("GOMEMLIMIT:-8MiB")
        );

        rollback_runtime_binaries_and_services(&paths, &[], &snapshot).unwrap();

        assert_eq!(
            fs::read_to_string(&cloudflared_drop_in).unwrap(),
            original_drop_in
        );
        assert_eq!(
            fs::read_to_string(&cloudflared_openrc).unwrap(),
            original_openrc
        );
        assert_eq!(fs::read_to_string(cloudflared_unit).unwrap(), original_unit);
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(
                &fs::metadata(&cloudflared_openrc).unwrap().permissions(),
            ) & 0o777,
            0o640
        );
        assert!(!systemd.join("xray.service.d/20-xp-memory.conf").exists());
        assert!(!systemd.join("xray.service.d").exists());
    }
}
