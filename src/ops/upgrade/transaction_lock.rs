use crate::ops::cli::ExitError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const TRANSACTION_LOCK_FILE: &str = "transaction.lock";
const ACQUISITION_GUARD_FILE: &str = "transaction.lock.guard";

#[derive(Debug)]
pub(super) struct UpgradeTransactionLock {
    path: PathBuf,
}

pub(super) fn begin(
    data_dir: &Path,
    is_real_upgrade: bool,
) -> Result<Option<UpgradeTransactionLock>, ExitError> {
    if !is_real_upgrade {
        return Ok(None);
    }
    if current_process_owns_lock(data_dir)? {
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
        let _guard = AcquisitionGuard::acquire(&dir)?;
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

struct AcquisitionGuard(fs::File);

impl AcquisitionGuard {
    fn acquire(dir: &Path) -> Result<Self, ExitError> {
        let file = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(ACQUISITION_GUARD_FILE))
            .map_err(|error| {
                ExitError::new(
                    7,
                    format!("service_error: create upgrade lock guard: {error}"),
                )
            })?;
        // Safety: `flock` operates only on this process's guard-file descriptor.
        if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_EX) } != 0 {
            return Err(ExitError::new(
                7,
                format!(
                    "service_error: lock upgrade transaction guard: {}",
                    io::Error::last_os_error()
                ),
            ));
        }
        Ok(Self(file))
    }
}

impl Drop for AcquisitionGuard {
    fn drop(&mut self) {
        // Safety: unlocks only this process's guard-file descriptor.
        let _ = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&self.0), libc::LOCK_UN) };
    }
}

impl Drop for UpgradeTransactionLock {
    fn drop(&mut self) {
        release_path(&self.path);
    }
}

pub(super) fn release(data_dir: &Path) {
    let path = crate::upgrade_job::upgrade_dir(data_dir).join(TRANSACTION_LOCK_FILE);
    release_path(&path);
}

fn release_path(path: &Path) {
    match lock_owner_pid(path) {
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

fn current_process_owns_lock(data_dir: &Path) -> Result<bool, ExitError> {
    let path = crate::upgrade_job::upgrade_dir(data_dir).join(TRANSACTION_LOCK_FILE);
    if !path.exists() {
        return Ok(false);
    }
    Ok(lock_owner_pid(&path)? == Some(current_pid()))
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
    use super::{TRANSACTION_LOCK_FILE, UpgradeTransactionLock, begin, release};

    #[test]
    fn rejects_a_second_live_upgrade_transaction() {
        let tmp = tempfile::tempdir().unwrap();
        let _lock = UpgradeTransactionLock::acquire(tmp.path()).unwrap();
        let error = UpgradeTransactionLock::acquire(tmp.path()).unwrap_err();
        assert_eq!(error.code, 3);
        assert_eq!(error.message, "upgrade_in_progress");
    }

    #[test]
    fn resumed_upgrade_without_a_lock_acquires_one() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = begin(tmp.path(), true).unwrap();
        assert!(lock.is_some());
    }

    #[test]
    fn nested_upgrade_inherits_the_current_process_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let _lock = UpgradeTransactionLock::acquire(tmp.path()).unwrap();
        assert!(begin(tmp.path(), true).unwrap().is_none());
    }

    #[test]
    fn reclaims_invalid_pid_locks() {
        let tmp = tempfile::tempdir().unwrap();
        let upgrade_dir = tmp.path().join("upgrade");
        std::fs::create_dir_all(&upgrade_dir).unwrap();
        let path = upgrade_dir.join(TRANSACTION_LOCK_FILE);
        for value in ["0", "-1", "not-a-pid"] {
            std::fs::write(&path, format!("{value}\n")).unwrap();
            let lock = UpgradeTransactionLock::acquire(tmp.path()).unwrap();
            drop(lock);
        }
    }

    #[test]
    fn foreign_resume_cannot_release_another_transaction_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let upgrade_dir = tmp.path().join("upgrade");
        std::fs::create_dir_all(&upgrade_dir).unwrap();
        let path = upgrade_dir.join(TRANSACTION_LOCK_FILE);
        std::fs::write(&path, "1\n").unwrap();

        assert!(begin(tmp.path(), true).is_err());
        release(tmp.path());
        assert!(path.exists());
    }

    #[test]
    fn dropping_a_replaced_lock_does_not_remove_the_foreign_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = UpgradeTransactionLock::acquire(tmp.path()).unwrap();
        let path = tmp.path().join("upgrade").join(TRANSACTION_LOCK_FILE);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, format!("{}\n", std::process::id() + 1)).unwrap();

        drop(lock);
        assert!(path.exists());
    }
}
