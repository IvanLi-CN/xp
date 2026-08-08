use super::{
    GitHubRelease, ResumeContext, UPGRADE_RESUME_API_BASE, UPGRADE_RESUME_REPO,
    UPGRADE_RESUME_SERVICE_PHASE_COMPLETE, UPGRADE_RESUME_TAG, UPGRADE_RESUME_XP_OPS_BACKUP,
    UPGRADE_RESUME_XP_OPS_DEST,
};
use crate::ops::Paths;
use crate::ops::cli::{ExitError, UpgradeArgs};
use crate::ops::upgrade_artifacts::cleanup_managed_artifacts_excluding;
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

const UPGRADE_RESUME_RUNNER: &str = "XP_OPS_UPGRADE_RESUME_RUNNER";

pub(super) fn resume_with_upgraded_xp_ops(
    paths: &Paths,
    args: &UpgradeArgs,
    release: &GitHubRelease,
    repository: &str,
    api_base: &str,
    xp_ops_dest: &Path,
    xp_ops_backup: &Path,
) -> Result<(), ExitError> {
    let mut command = Command::new(xp_ops_dest);
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
        .env(UPGRADE_RESUME_XP_OPS_DEST, xp_ops_dest)
        .env(UPGRADE_RESUME_XP_OPS_BACKUP, xp_ops_backup)
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
    if let Err(error) =
        cleanup_managed_artifacts_excluding(paths, &[&resume.xp_ops_dest], &[&resume.xp_ops_backup])
    {
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
    if let Err(error) = finish_upgrade_runner_status(data_dir) {
        return recover_after_complete_phase_failure(paths, data_dir, resume, error);
    }
    if let Err(error) = std::fs::remove_file(&resume.xp_ops_backup) {
        return recover_after_complete_phase_failure(
            paths,
            data_dir,
            resume,
            ExitError::new(7, format!("service_error: cleanup xp-ops backup: {error}")),
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
            UPGRADE_RESUME_SERVICE_PHASE_COMPLETE,
            UPGRADE_RESUME_RUNNER,
        ] {
            std::env::remove_var(key);
        }
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
