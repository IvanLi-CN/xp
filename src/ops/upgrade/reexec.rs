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
    if let Err(error) = remove_transaction_backups(resume) {
        return recover_after_complete_phase_failure(
            paths,
            data_dir,
            resume,
            ExitError::new(
                7,
                format!("service_error: cleanup transaction backups: {error}"),
            ),
        );
    }
    if let Err(error) = finish_upgrade_runner_status(data_dir) {
        tracing::warn!(
            error = %error.message,
            "upgrade committed but could not write runner success status"
        );
    }
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
            Ok((path.to_path_buf(), std::fs::read(path)?))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    for (index, (path, _)) in snapshots.iter().enumerate() {
        if let Err(error) = remove(path) {
            for (restore_path, contents) in snapshots.iter().take(index) {
                if !restore_path.exists() {
                    let _ = std::fs::write(restore_path, contents);
                }
            }
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::remove_transaction_backup_paths;

    #[test]
    fn restores_removed_backups_when_later_delete_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("xp-ops.bak.test");
        let second = tmp.path().join("xp.bak.test");
        std::fs::write(&first, b"xp-ops-old").unwrap();
        std::fs::write(&second, b"xp-old").unwrap();
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
        assert_eq!(std::fs::read(&first).unwrap(), b"xp-ops-old");
        assert_eq!(std::fs::read(&second).unwrap(), b"xp-old");
    }
}

pub(super) fn mark_upgrade_runner_resume() {
    // Safety: env vars are process-local and no other threads mutate them in `xp-ops`.
    unsafe { std::env::set_var(UPGRADE_RESUME_RUNNER, "1") }
}

fn finish_upgrade_runner_status(data_dir: &Path) -> Result<(), ExitError> {
    if !matches!(std::env::var(UPGRADE_RESUME_RUNNER).as_deref(), Ok("1")) {
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
