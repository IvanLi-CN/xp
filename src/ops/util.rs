use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    DryRun,
    Real,
}

pub fn is_test_root(root: &Path) -> bool {
    root != Path::new("/")
}

pub fn ensure_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

pub fn write_bytes_if_changed(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    if let Ok(existing) = fs::read(path)
        && existing == bytes
    {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = tmp_path_next_to(path);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(true)
}

pub fn write_string_if_changed(path: &Path, content: &str) -> io::Result<bool> {
    write_bytes_if_changed(path, content.as_bytes())
}

#[cfg(unix)]
pub fn chmod(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
pub fn chmod(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

pub fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.exists()
    }
}

pub fn tmp_path_next_to(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("tmp"));
    parent.join(format!(
        ".{}.tmp.{}",
        file.to_string_lossy(),
        std::process::id()
    ))
}
