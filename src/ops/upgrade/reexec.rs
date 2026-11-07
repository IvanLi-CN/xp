use super::{
    GitHubRelease, ResumeContext, UPGRADE_RESUME_API_BASE, UPGRADE_RESUME_REPO,
    UPGRADE_RESUME_SERVICE_BACKUPS, UPGRADE_RESUME_SERVICE_PHASE_COMPLETE, UPGRADE_RESUME_TAG,
    UPGRADE_RESUME_XP_OPS_BACKUP, UPGRADE_RESUME_XP_OPS_DEST,
};
use crate::ops::Paths;
use crate::ops::cli::{ExitError, UpgradeArgs};
use crate::ops::upgrade_artifacts::cleanup_managed_artifacts_excluding;
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

const UPGRADE_RESUME_RUNNER: &str = "XP_OPS_UPGRADE_RESUME_RUNNER";

pub(super) struct ReexecTransaction<'a> {
    pub(super) xp_ops_dest: &'a Path,
    pub(super) xp_ops_backup: &'a Path,
    pub(super) service_backups: &'a [super::managed_runtimes::RuntimeBinaryBackup],
}

pub(super) fn resume_with_upgraded_xp_ops(
    paths: &Paths,
    args: &UpgradeArgs,
    release: &GitHubRelease,
    repository: &str,
    api_base: &str,
    transaction: ReexecTransaction<'_>,
) -> Result<(), ExitError> {
    let mut command = Command::new(transaction.xp_ops_dest);
    command
        .arg("--root")
        .arg(paths.root())
        .arg("upgrade")
        .arg("--version")
        .arg(&release.tag_name)
        .arg("--repo")
        .arg(repository)
        .arg("--data-dir")
        .arg(&args.data_dir)
        .env(UPGRADE_RESUME_TAG, &release.tag_name)
        .env(UPGRADE_RESUME_REPO, repository)
        .env(UPGRADE_RESUME_API_BASE, api_base)
        .env(UPGRADE_RESUME_XP_OPS_DEST, transaction.xp_ops_dest)
        .env(UPGRADE_RESUME_XP_OPS_BACKUP, transaction.xp_ops_backup)
        .env(
            UPGRADE_RESUME_SERVICE_BACKUPS,
            serde_json::to_string(transaction.service_backups).map_err(|error| {
                ExitError::new(
                    7,
                    format!("service_error: serialize service rollback context: {error}"),
                )
            })?,
        )
        .env(UPGRADE_RESUME_SERVICE_PHASE_COMPLETE, "1");
    if args.allow_internal_auth_v2_cutover {
        command.arg("--allow-internal-auth-v2-cutover");
    }
    let error = command.exec();
    Err(ExitError::new(
        7,
        format!("install_failed: self-reexec xp-ops: {error}"),
    ))
}

pub(super) fn finish_reexeced_upgrade(
    paths: &Paths,
    data_dir: &Path,
    resume: &ResumeContext,
) -> Result<(), ExitError> {
    finish_reexeced_upgrade_with_cleanup(paths, data_dir, resume, remove_transaction_backups)
}

fn finish_reexeced_upgrade_with_cleanup<F>(
    paths: &Paths,
    data_dir: &Path,
    resume: &ResumeContext,
    cleanup: F,
) -> Result<(), ExitError>
where
    F: FnOnce(&ResumeContext) -> std::io::Result<()>,
{
    if matches!(std::env::var(UPGRADE_RESUME_RUNNER).as_deref(), Ok("1"))
        && let Err(error) = ensure_runner_status_writable(data_dir)
    {
        return recover_after_complete_phase_failure(
            paths,
            data_dir,
            resume,
            ExitError::new(7, format!("service_error: prepare upgrade status: {error}")),
        );
    }
    if let Err(error) = cleanup_managed_artifacts_excluding(
        paths,
        &[&resume.xp_ops_dest],
        &transaction_backups(resume),
    ) {
        return recover_after_complete_phase_failure(
            paths,
            data_dir,
            resume,
            ExitError::new(
                7,
                format!("service_error: cleanup upgrade artifacts: {error}"),
            ),
        );
    }
    if let Err(error) =
        super::failure::persist_committed_cleanup_guard(data_dir, &resume.release.tag)
    {
        return recover_after_complete_phase_failure(
            paths,
            data_dir,
            resume,
            ExitError::new(
                7,
                format!("service_error: persist backup cleanup guard: {error}"),
            ),
        );
    }
    // Activation is complete. Unlink is irreversible across multiple files; never roll
    // installed binaries back through backups that may already have been removed.
    if let Err(error) = cleanup(resume) {
        let error = super::failure::committed_cleanup_error(error);
        super::failure::write_upgrade_diagnostics(
            data_dir,
            &resume.release.tag,
            &HashMap::new(),
            &error,
        );
        write_upgrade_runner_failure(data_dir, &error);
        super::transaction_lock::release(&paths.map_abs(data_dir));
        clear_upgrade_resume_env();
        return Err(error);
    }
    if let Err(error) = finish_upgrade_runner_status(data_dir) {
        super::failure::write_upgrade_diagnostics(
            data_dir,
            &resume.release.tag,
            &HashMap::new(),
            &error,
        );
        super::transaction_lock::release(&paths.map_abs(data_dir));
        clear_upgrade_resume_env();
        return Err(error);
    }
    super::transaction_lock::release(&paths.map_abs(data_dir));
    clear_upgrade_resume_env();
    super::failure::clear_upgrade_diagnostics(data_dir);
    Ok(())
}

fn recover_after_complete_phase_failure(
    paths: &Paths,
    data_dir: &Path,
    resume: &ResumeContext,
    error: ExitError,
) -> Result<(), ExitError> {
    let error = match super::managed_runtimes::rollback_complete_phase_binaries(
        paths,
        &resume.service_backups,
    ) {
        Ok(()) => error,
        Err(rollback) => ExitError::new(
            rollback.code,
            format!(
                "{}; service binary rollback failed: {}",
                error.message, rollback.message
            ),
        ),
    };
    let error = match super::failure::rollback_xp_ops_after_resumed_failure(
        &resume.xp_ops_dest,
        &resume.xp_ops_backup,
        error,
    ) {
        Err(error) => error,
        Ok(()) => unreachable!("xp-ops rollback helper must return an error"),
    };
    let error = super::failure::cleanup_after_upgrade_failure(paths, &[&resume.xp_ops_dest], error);
    super::failure::write_upgrade_diagnostics(
        data_dir,
        &resume.release.tag,
        &HashMap::new(),
        &error,
    );
    write_upgrade_runner_failure(data_dir, &error);
    super::transaction_lock::release(&paths.map_abs(data_dir));
    Err(error)
}

pub(super) fn clear_upgrade_resume_env() {
    // Safety: env vars are process-local and no other threads mutate them in `xp-ops`.
    unsafe {
        for key in [
            UPGRADE_RESUME_TAG,
            UPGRADE_RESUME_REPO,
            UPGRADE_RESUME_API_BASE,
            UPGRADE_RESUME_XP_OPS_DEST,
            UPGRADE_RESUME_XP_OPS_BACKUP,
            UPGRADE_RESUME_SERVICE_BACKUPS,
            UPGRADE_RESUME_SERVICE_PHASE_COMPLETE,
            UPGRADE_RESUME_RUNNER,
        ] {
            std::env::remove_var(key);
        }
    }
}

fn transaction_backups(resume: &ResumeContext) -> Vec<&Path> {
    std::iter::once(resume.xp_ops_backup.as_path())
        .chain(
            resume
                .service_backups
                .iter()
                .filter_map(|backup| backup.backup.as_deref()),
        )
        .collect()
}

fn remove_transaction_backups(resume: &ResumeContext) -> std::io::Result<()> {
    remove_transaction_backup_paths(transaction_backups(resume), |path| {
        std::fs::remove_file(path)
    })
}

struct TransactionBackupSnapshot {
    path: std::path::PathBuf,
    _file: std::fs::File,
}

fn remove_transaction_backup_paths<F>(backups: Vec<&Path>, mut remove: F) -> std::io::Result<()>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    let snapshots = backups
        .into_iter()
        .map(|path| {
            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.file_type().is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "transaction backup is not a regular file: {}",
                        path.display()
                    ),
                ));
            }
            // Validate all original inodes without reading binary contents into RAM.
            let file = std::fs::File::open(path)?;
            let opened = file.metadata()?;
            use std::os::unix::fs::MetadataExt;
            if !opened.is_file() || opened.dev() != metadata.dev() || opened.ino() != metadata.ino()
            {
                return Err(std::io::Error::other(
                    "transaction backup changed while opening",
                ));
            }
            Ok(TransactionBackupSnapshot {
                path: path.to_path_buf(),
                _file: file,
            })
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    for snapshot in &snapshots {
        remove(&snapshot.path)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        ensure_runner_status_writable, finish_upgrade_runner_status_for_runner,
        remove_transaction_backup_paths,
    };
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn cleanup_failure_preserves_every_remaining_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("xp-ops.bak.test");
        let second = tmp.path().join("xp.bak.test");
        std::fs::write(&first, b"xp-ops-old").unwrap();
        std::fs::write(&second, b"xp-old").unwrap();
        std::fs::set_permissions(&first, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut calls = 0;

        let error = remove_transaction_backup_paths(vec![&first, &second], |path| {
            calls += 1;
            if calls == 2 {
                return Err(std::io::Error::other("injected deletion failure"));
            }
            std::fs::remove_file(path)
        })
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::Other);
        assert!(!first.exists());
        assert_eq!(std::fs::read(&second).unwrap(), b"xp-old");
        assert_eq!(
            std::fs::metadata(&second).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn backup_cleanup_is_bounded_for_large_files() {
        const CHILD: &str = "XP_OPS_BACKUP_MEMORY_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "backup_cleanup_is_bounded_for_large_files",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        // Limit only the isolated child; the parent test suite keeps its normal limits.
        let limit = libc::rlimit {
            rlim_cur: 64 * 1024 * 1024,
            rlim_max: 64 * 1024 * 1024,
        };
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_DATA, &limit) }, 0);
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("xp.bak.large");
        let second = tmp.path().join("xp-ops.bak.large");
        for path in [&first, &second] {
            std::fs::File::create(path)
                .unwrap()
                .set_len(128 * 1024 * 1024)
                .unwrap();
        }
        remove_transaction_backup_paths(vec![&first, &second], |path| std::fs::remove_file(path))
            .unwrap();
        assert!(!first.exists() && !second.exists());
    }

    #[test]
    fn backup_cleanup_rejects_symlinks_before_deleting_any_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("xp.bak.test");
        let target = tmp.path().join("unrelated");
        let link = tmp.path().join("xp-ops.bak.link");
        std::fs::write(&first, b"xp-old").unwrap();
        std::fs::write(&target, b"unrelated").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error =
            remove_transaction_backup_paths(vec![&first, &link], |path| std::fs::remove_file(path))
                .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read(&first).unwrap(), b"xp-old");
        assert_eq!(std::fs::read(&target).unwrap(), b"unrelated");
    }

    #[test]
    fn committed_cleanup_failure_never_rolls_back_ready_binaries() {
        const CHILD: &str = "XP_OPS_COMMITTED_CLEANUP_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "committed_cleanup_failure_never_rolls_back_ready_binaries",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::ops::Paths::new(tmp.path().to_path_buf());
        let xp = paths.usr_local_bin_xp();
        let ops = paths.usr_local_bin_xp_ops();
        std::fs::create_dir_all(xp.parent().unwrap()).unwrap();
        std::fs::write(&xp, b"xp-ready").unwrap();
        std::fs::write(&ops, b"ops-ready").unwrap();
        let xp_backup = xp.with_extension("bak.test");
        let ops_backup = ops.with_extension("bak.test");
        std::fs::write(&xp_backup, b"xp-old").unwrap();
        std::fs::write(&ops_backup, b"ops-old").unwrap();
        let resume = super::ResumeContext {
            release: super::super::LockedRelease {
                owner: "test".into(),
                repo: "xp".into(),
                api_base: "https://invalid".into(),
                tag: xp_test_fixtures::release_tag().into(),
            },
            xp_ops_dest: ops.clone(),
            xp_ops_backup: ops_backup.clone(),
            service_backups: vec![super::super::managed_runtimes::RuntimeBinaryBackup {
                dest: xp.clone(),
                backup: Some(xp_backup.clone()),
            }],
            service_phase_complete: true,
        };
        let error =
            super::finish_reexeced_upgrade_with_cleanup(&paths, tmp.path(), &resume, |_| {
                let guard =
                    std::fs::read_to_string(tmp.path().join("upgrade/diagnostics.json")).unwrap();
                assert!(guard.contains("cleanup_incomplete: committed_upgrade:"));
                std::fs::remove_file(&ops_backup)?;
                Err(std::io::Error::other("injected later unlink failure"))
            })
            .unwrap_err();
        assert!(
            error
                .message
                .starts_with("cleanup_incomplete: committed_upgrade:")
        );
        assert_eq!(std::fs::read(&xp).unwrap(), b"xp-ready");
        assert_eq!(std::fs::read(&ops).unwrap(), b"ops-ready");
        assert_eq!(std::fs::read(&xp_backup).unwrap(), b"xp-old");
        assert!(!ops_backup.exists());
        assert!(
            super::super::failure::preflight_upgrade(&paths, tmp.path(), &xp, &ops)
                .unwrap_err()
                .message
                .contains("manual recovery is required")
        );
        assert!(xp_backup.exists());

        // A new fixture transaction whose guard cannot be persisted must retain the
        // original pre-commit rollback path, without ever invoking backup cleanup.
        std::fs::write(&ops_backup, b"ops-old").unwrap();
        std::fs::create_dir(tmp.path().join("upgrade/diagnostics.json.tmp")).unwrap();
        let error =
            super::finish_reexeced_upgrade_with_cleanup(&paths, tmp.path(), &resume, |_| {
                panic!("must not unlink backups before the cleanup guard is durable")
            })
            .unwrap_err();
        assert!(error.message.contains("persist backup cleanup guard"));
        assert_eq!(std::fs::read(&xp).unwrap(), b"xp-old");
        assert_eq!(std::fs::read(&ops).unwrap(), b"ops-old");
    }

    #[test]
    fn runner_status_preflight_rejects_a_non_file_temporary_path() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("upgrade/status.json.tmp")).unwrap();
        let error = ensure_runner_status_writable(tmp.path()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn runner_status_completion_reports_write_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let upgrade_dir = tmp.path().join("upgrade");
        std::fs::create_dir_all(&upgrade_dir).unwrap();
        let request = crate::upgrade_job::UpgradeRequest {
            target_tag: "v0.2.0".to_string(),
            // Other integration tests can temporarily install a root-controlled
            // repository in the process environment. This test exercises the
            // status-write failure path, so keep its request valid in either state.
            repo: std::env::var("XP_OPS_GITHUB_REPO").ok(),
            requested_at: "2026-08-08T00:00:00Z".to_string(),
        };
        std::fs::write(
            crate::upgrade_job::request_path(tmp.path()),
            serde_json::to_vec(&request).unwrap(),
        )
        .unwrap();
        std::fs::create_dir(upgrade_dir.join("status.json.tmp")).unwrap();

        let error = finish_upgrade_runner_status_for_runner(tmp.path(), true).unwrap_err();
        assert_eq!(error.code, 7);
        assert!(error.message.contains("write upgrade status"));
    }
}

pub(super) fn mark_upgrade_runner_resume() {
    // Safety: env vars are process-local and no other threads mutate them in `xp-ops`.
    unsafe { std::env::set_var(UPGRADE_RESUME_RUNNER, "1") }
}

fn finish_upgrade_runner_status(data_dir: &Path) -> Result<(), ExitError> {
    finish_upgrade_runner_status_for_runner(
        data_dir,
        matches!(std::env::var(UPGRADE_RESUME_RUNNER).as_deref(), Ok("1")),
    )
}

fn finish_upgrade_runner_status_for_runner(
    data_dir: &Path,
    is_runner: bool,
) -> Result<(), ExitError> {
    if !is_runner {
        return Ok(());
    }
    let request = crate::upgrade_job::prepare_runner_request(data_dir, super::DEFAULT_GITHUB_REPO)?;
    let status = crate::upgrade_job::status_for_runner_finish(&request, Ok(()));
    crate::upgrade_job::write_status(data_dir, &status)
        .map_err(|error| ExitError::new(7, format!("service_error: write upgrade status: {error}")))
}

fn ensure_runner_status_writable(data_dir: &Path) -> std::io::Result<()> {
    let dir = crate::upgrade_job::upgrade_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let tmp = crate::upgrade_job::status_path(data_dir).with_extension("json.tmp");
    match std::fs::symlink_metadata(&tmp) {
        Ok(metadata) if metadata.file_type().is_dir() || metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "upgrade status temporary path is not a regular file",
            ));
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::File::create(&tmp)?;
    std::fs::remove_file(tmp)
}

fn write_upgrade_runner_failure(data_dir: &Path, error: &ExitError) {
    if !matches!(std::env::var(UPGRADE_RESUME_RUNNER).as_deref(), Ok("1")) {
        return;
    }
    if let Ok(request) =
        crate::upgrade_job::prepare_runner_request(data_dir, super::DEFAULT_GITHUB_REPO)
    {
        let status = crate::upgrade_job::status_for_runner_finish(&request, Err(error));
        let _ = crate::upgrade_job::write_status(data_dir, &status);
    }
}
