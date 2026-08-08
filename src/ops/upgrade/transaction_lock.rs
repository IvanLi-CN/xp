use crate::ops::cli::ExitError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const TRANSACTION_LOCK_FILE: &str = "transaction.lock";

#[derive(Debug)]
pub(super) struct UpgradeTransactionLock {
    path: PathBuf,
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
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(error = %error, "could not release upgrade transaction lock");
    }
}

fn lock_owner_is_running(path: &Path) -> Result<bool, ExitError> {
    let raw = fs::read_to_string(path)
        .map_err(|error| ExitError::new(7, format!("service_error: read upgrade lock: {error}")))?;
    let Ok(pid) = raw.trim().parse::<i32>() else {
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

#[cfg(test)]
mod tests {
    use super::UpgradeTransactionLock;

    #[test]
    fn rejects_a_second_live_upgrade_transaction() {
        let tmp = tempfile::tempdir().unwrap();
        let _lock = UpgradeTransactionLock::acquire(tmp.path()).unwrap();
        let error = UpgradeTransactionLock::acquire(tmp.path()).unwrap_err();
        assert_eq!(error.code, 3);
        assert_eq!(error.message, "upgrade_in_progress");
    }
}
