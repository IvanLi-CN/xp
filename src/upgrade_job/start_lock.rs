use super::*;

#[cfg(unix)]
use std::os::fd::AsRawFd;

pub(super) struct StartLock {
    file: File,
}

impl StartLock {
    pub(super) fn acquire(data_dir: &Path) -> Result<Self, UpgradeStartError> {
        fs::create_dir_all(upgrade_dir(data_dir))?;
        let file = File::options()
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path(data_dir))?;
        #[cfg(unix)]
        {
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result == 0 {
                return Ok(Self { file });
            }
            let err = io::Error::last_os_error();
            if matches!(
                err.raw_os_error(),
                Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
            ) {
                return Err(UpgradeStartError::Active);
            }
            Err(UpgradeStartError::Io(err))
        }
        #[cfg(not(unix))]
        Ok(Self { file })
    }
}

impl Drop for StartLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
