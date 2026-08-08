use crate::ops::cli::ExitError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const TRANSACTION_LOCK_FILE: &str = "transaction.lock";

#[derive(Debug)]
pub(super) struct UpgradeTransactionLock {
    path: PathBuf,
}

pub(super) fn begin(
    data_dir: &Path,
    is_real_upgrade: bool,
    is_resumed_upgrade: bool,
) -> Result<Option<UpgradeTransactionLock>, ExitError> {
    if !is_real_upgrade {
        return Ok(None);
    }
    if is_resumed_upgrade {
        verify_resume_owner(data_dir)?;
        return Ok(None);
    }
    UpgradeTransactionLock::acquire(data_dir).map(Some)
}

impl UpgradeTransactionLock {
    pub(super) fn acquire(data_dir: &Path) -> Result<Self, ExitError> {
        let dir = crate::upgrade_job::upgrade_dir(data_dir);
        fs::create_dir_all(&dir).map_err(|error| {
            ExitError::new(7, format!("service_error: create upgrade lock: {error}"))
        })?;
        let path = dir.join(TRANSACTION_LOCK_FILE);
        for _ in 0..2 {
            match fs::File::options().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    use std::io::Write;
                    writeln!(file, "{}", std::process::id()).map_err(|error| {
                        ExitError::new(7, format!("service_error: write upgrade lock: {error}"))
                    })?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if lock_owner_is_running(&path)? {
                        return Err(ExitError::new(3, "upgrade_in_progress"));
                    }
                    fs::remove_file(&path).map_err(|error| {
                        ExitError::new(
                            7,
                            format!("service_error: remove stale upgrade lock: {error}"),
                        )
                    })?;
                }
                Err(error) => {
                    return Err(ExitError::new(
                        7,
                        format!("service_error: create upgrade lock: {error}"),
                    ));
                }
            }
        }
        Err(ExitError::new(3, "upgrade_in_progress"))
    }
}

impl Drop for UpgradeTransactionLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(super) fn release(data_dir: &Path) {
    let path = crate::upgrade_job::upgrade_dir(data_dir).join(TRANSACTION_LOCK_FILE);
    match lock_owner_pid(&path) {
        Ok(Some(pid)) if pid == current_pid() => {
            if let Err(error) = fs::remove_file(path) {
                tracing::warn!(error = %error, "could not release upgrade transaction lock");
            }
        }
        Ok(Some(_)) => tracing::warn!(
            "refusing to release an upgrade transaction lock owned by another process"
        ),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(error = %error.message, "could not inspect upgrade transaction lock")
        }
    }
}

pub(super) fn verify_resume_owner(data_dir: &Path) -> Result<(), ExitError> {
    let path = crate::upgrade_job::upgrade_dir(data_dir).join(TRANSACTION_LOCK_FILE);
    if lock_owner_pid(&path)? == Some(current_pid()) {
        return Ok(());
    }
    Err(ExitError::new(
        3,
        "invalid_args: resumed upgrade lock is not owned by current process",
    ))
}

fn lock_owner_is_running(path: &Path) -> Result<bool, ExitError> {
    let Some(pid) = lock_owner_pid(path)? else {
        return Ok(false);
    };
    // Safety: kill with signal zero only probes a local process; it sends no signal.
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return Ok(true);
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Ok(true),
    }
}

fn lock_owner_pid(path: &Path) -> Result<Option<i32>, ExitError> {
    let raw = fs::read_to_string(path)
        .map_err(|error| ExitError::new(7, format!("service_error: read upgrade lock: {error}")))?;
    let Ok(pid) = raw.trim().parse::<i32>() else {
        return Ok(None);
    };
    if pid <= 0 {
        return Ok(None);
    }
    Ok(Some(pid))
}

fn current_pid() -> i32 {
    i32::try_from(std::process::id()).unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::{TRANSACTION_LOCK_FILE, UpgradeTransactionLock, release, verify_resume_owner};

    #[test]
    fn rejects_a_second_live_upgrade_transaction() {
        let tmp = tempfile::tempdir().unwrap();
        let _lock = UpgradeTransactionLock::acquire(tmp.path()).unwrap();
        let error = UpgradeTransactionLock::acquire(tmp.path()).unwrap_err();
        assert_eq!(error.code, 3);
        assert_eq!(error.message, "upgrade_in_progress");
    }

    #[test]
    fn reclaims_a_zero_pid_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let upgrade_dir = tmp.path().join("upgrade");
        std::fs::create_dir_all(&upgrade_dir).unwrap();
        std::fs::write(upgrade_dir.join(TRANSACTION_LOCK_FILE), "0\n").unwrap();
        let lock = UpgradeTransactionLock::acquire(tmp.path()).unwrap();
        drop(lock);
    }

    #[test]
    fn foreign_resume_cannot_release_another_transaction_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let upgrade_dir = tmp.path().join("upgrade");
        std::fs::create_dir_all(&upgrade_dir).unwrap();
        let path = upgrade_dir.join(TRANSACTION_LOCK_FILE);
        std::fs::write(&path, format!("{}\n", std::process::id() + 1)).unwrap();

        assert!(verify_resume_owner(tmp.path()).is_err());
        release(tmp.path());
        assert!(path.exists());
    }
}
