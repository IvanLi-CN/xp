use super::{
    GitHubRelease, ResumeContext, UPGRADE_RESUME_API_BASE, UPGRADE_RESUME_REPO,
    UPGRADE_RESUME_SERVICE_PHASE_COMPLETE, UPGRADE_RESUME_TAG, UPGRADE_RESUME_XP_OPS_BACKUP,
    UPGRADE_RESUME_XP_OPS_DEST,
};
use crate::ops::Paths;
use crate::ops::cli::{ExitError, UpgradeArgs};
use crate::ops::upgrade_artifacts::cleanup_managed_artifacts_for;
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

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
    match cleanup_managed_artifacts_for(paths, &[&resume.xp_ops_dest]) {
        Ok(_) => {
            clear_upgrade_resume_env();
            super::failure::clear_upgrade_diagnostics(data_dir);
            Ok(())
        }
        Err(error) => {
            let error = ExitError::new(
                7,
                format!("service_error: cleanup upgrade artifacts: {error}"),
            );
            super::failure::write_upgrade_diagnostics(
                data_dir,
                &resume.release.tag,
                &HashMap::new(),
                &error,
            );
            Err(error)
        }
    }
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
        ] {
            std::env::remove_var(key);
        }
    }
}
