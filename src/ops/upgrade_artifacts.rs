use crate::ops::paths::Paths;
use serde::Serialize;
use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const MIN_UPGRADE_FREE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeStorageVolume {
    pub path: String,
    pub available_bytes: u64,
    pub reclaimable_bytes: u64,
    pub required_bytes: u64,
    pub sufficient_after_cleanup: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeStorage {
    pub install: UpgradeStorageVolume,
    pub workspace: UpgradeStorageVolume,
    pub cleanup_required: bool,
}

pub(crate) fn workspace_path(paths: &Paths) -> PathBuf {
    paths.map_abs(Path::new("/tmp/xp-ops"))
}

pub(crate) fn assess_upgrade_storage(paths: &Paths) -> io::Result<UpgradeStorage> {
    assess_upgrade_storage_for(paths, &[])
}

pub(crate) fn ensure_upgrade_space_for(paths: &Paths, extra: &[&Path]) -> Result<(), String> {
    let storage = assess_upgrade_storage_for(paths, extra).map_err(|error| error.to_string())?;
    if storage.install.sufficient_after_cleanup && storage.workspace.sufficient_after_cleanup {
        return Ok(());
    }
    Err(format!(
        "insufficient_upgrade_space: require at least {} MiB free after cleanup",
        MIN_UPGRADE_FREE_BYTES / 1024 / 1024
    ))
}

fn assess_upgrade_storage_for(paths: &Paths, extra: &[&Path]) -> io::Result<UpgradeStorage> {
    let artifacts = managed_artifacts(paths, extra)?;
    let workspace = workspace_path(paths);
    let workspace_reclaimable = workspace_reclaimable(paths.root(), &workspace)?;
    let install = managed_installation_binaries(paths)
        .into_iter()
        .chain(extra.iter().map(|path| path.to_path_buf()))
        .map(|path| volume_for(&path, &artifacts, &workspace, workspace_reclaimable))
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .min_by_key(|volume| {
            volume
                .available_bytes
                .saturating_add(volume.reclaimable_bytes)
        })
        .ok_or_else(|| io::Error::other("no managed installation paths"))?;
    let workspace_volume = volume_for(&workspace, &artifacts, &workspace, workspace_reclaimable)?;
    Ok(UpgradeStorage {
        cleanup_required: !artifacts.is_empty() || workspace_reclaimable > 0,
        install,
        workspace: workspace_volume,
    })
}

pub(crate) fn cleanup_managed_artifacts_for(paths: &Paths, extra: &[&Path]) -> io::Result<u64> {
    cleanup_managed_artifacts_excluding(paths, extra, &[])
}

pub(crate) fn has_managed_backup_for(paths: &Paths, extra: &[&Path]) -> io::Result<bool> {
    Ok(managed_artifacts(paths, extra)?.iter().any(|artifact| {
        artifact
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".bak."))
    }))
}

pub(crate) fn cleanup_managed_artifacts_excluding(
    paths: &Paths,
    extra: &[&Path],
    excluded: &[&Path],
) -> io::Result<u64> {
    let mut reclaimed = 0u64;
    for artifact in managed_artifacts(paths, extra)? {
        if excluded.iter().any(|path| artifact == *path) {
            continue;
        }
        if let Ok(metadata) = fs::symlink_metadata(&artifact)
            && metadata.file_type().is_file()
        {
            reclaimed = reclaimed.saturating_add(metadata.len());
            fs::remove_file(artifact)?;
        }
    }
    reclaimed = reclaimed.saturating_add(cleanup_workspace_from(
        paths.root(),
        &workspace_path(paths),
    )?);
    Ok(reclaimed)
}

pub(crate) fn cleanup_workspace(workspace: &Path) -> io::Result<u64> {
    let root = workspace.parent().unwrap_or(workspace);
    cleanup_workspace_from(root, workspace)
}

fn cleanup_workspace_from(root: &Path, workspace: &Path) -> io::Result<u64> {
    ensure_no_symlinked_ancestors_from(root, workspace)?;
    let metadata = match fs::symlink_metadata(workspace) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing symlinked upgrade workspace",
        ));
    }
    if !metadata.file_type().is_dir() {
        return Ok(0);
    }
    let reclaimed = directory_regular_file_bytes(workspace)?;
    fs::remove_dir_all(workspace)?;
    Ok(reclaimed)
}

pub(crate) fn managed_cloudflared_dest(paths: &Paths) -> PathBuf {
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

fn managed_installation_binaries(paths: &Paths) -> [PathBuf; 4] {
    [
        paths.usr_local_bin_xp(),
        paths.usr_local_bin_xp_ops(),
        paths.usr_local_bin_xray(),
        managed_cloudflared_dest(paths),
    ]
}

fn managed_artifact_binaries(paths: &Paths) -> [PathBuf; 5] {
    [
        paths.usr_local_bin_xp(),
        paths.usr_local_bin_xp_ops(),
        paths.usr_local_bin_xray(),
        paths.usr_local_bin_cloudflared(),
        paths.usr_bin_cloudflared(),
    ]
}

fn managed_artifacts(paths: &Paths, extra: &[&Path]) -> io::Result<Vec<PathBuf>> {
    let managed = managed_artifact_binaries(paths)
        .into_iter()
        .chain(extra.iter().map(|path| path.to_path_buf()));
    let mut artifacts = Vec::new();
    let mut seen_dirs = BTreeSet::new();
    for binary in managed {
        let Some(parent) = binary.parent() else {
            continue;
        };
        let Some(name) = binary.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let key = parent.to_path_buf();
        if !seen_dirs.insert((key.clone(), name.to_string())) {
            continue;
        }
        ensure_no_symlinked_ancestors_from(paths.root(), &key)?;
        match fs::symlink_metadata(&key) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "refusing symlinked managed artifact directory: {}",
                        key.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
        let entries = match fs::read_dir(&key) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if is_managed_artifact_name(file_name, name) && entry.file_type()?.is_file() {
                artifacts.push(entry.path());
            }
        }
    }
    Ok(artifacts)
}

fn is_managed_artifact_name(candidate: &str, binary: &str) -> bool {
    [
        format!("{binary}.bak."),
        format!("{binary}.failed."),
        format!(".{binary}.tmp."),
    ]
    .into_iter()
    .any(|prefix| {
        candidate
            .strip_prefix(&prefix)
            .is_some_and(|suffix| !suffix.is_empty())
    })
}

fn volume_for(
    path: &Path,
    artifacts: &[PathBuf],
    workspace: &Path,
    workspace_reclaimable: u64,
) -> io::Result<UpgradeStorageVolume> {
    let existing = existing_ancestor(path)?;
    let available_bytes = available_bytes(&existing)?;
    let artifact_reclaimable = artifacts
        .iter()
        .filter(|artifact| same_filesystem(&existing, artifact).unwrap_or(false))
        .filter_map(|artifact| fs::symlink_metadata(artifact).ok())
        .filter(|metadata| metadata.file_type().is_file())
        .fold(0u64, |total, metadata| total.saturating_add(metadata.len()));
    let reclaimable_bytes = artifact_reclaimable.saturating_add(
        same_filesystem(&existing, workspace)
            .ok()
            .filter(|same| *same)
            .map(|_| workspace_reclaimable)
            .unwrap_or(0),
    );
    let sufficient_after_cleanup =
        available_bytes.saturating_add(reclaimable_bytes) >= MIN_UPGRADE_FREE_BYTES;
    Ok(UpgradeStorageVolume {
        path: existing.display().to_string(),
        available_bytes,
        reclaimable_bytes,
        required_bytes: MIN_UPGRADE_FREE_BYTES,
        sufficient_after_cleanup,
    })
}

fn workspace_reclaimable(root: &Path, workspace: &Path) -> io::Result<u64> {
    ensure_no_symlinked_ancestors_from(root, workspace)?;
    match fs::symlink_metadata(workspace) {
        Ok(metadata) if metadata.file_type().is_dir() => directory_regular_file_bytes(workspace),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing symlinked upgrade workspace",
        )),
        Ok(_) => Ok(0),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error),
    }
}

fn directory_regular_file_bytes(dir: &Path) -> io::Result<u64> {
    let mut bytes = 0u64;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = entry.file_type()?;
        if metadata.is_file() {
            bytes = bytes.saturating_add(entry.metadata()?.len());
        } else if metadata.is_dir() {
            bytes = bytes.saturating_add(directory_regular_file_bytes(&entry.path())?);
        }
    }
    Ok(bytes)
}

fn existing_ancestor(path: &Path) -> io::Result<PathBuf> {
    path.ancestors()
        .find(|candidate| candidate.exists())
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no existing filesystem path"))
}

fn ensure_no_symlinked_ancestors_from(root: &Path, path: &Path) -> io::Result<()> {
    let ancestors = path
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(root))
        .collect::<Vec<_>>();
    if ancestors.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing managed path outside root: {}", path.display()),
        ));
    }
    for ancestor in ancestors.into_iter().rev() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "refusing path with symlinked ancestor: {}",
                        ancestor.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn same_filesystem(left: &Path, right: &Path) -> io::Result<bool> {
    Ok(filesystem_id(left)? == filesystem_id(&existing_ancestor(right)?)?)
}

fn filesystem_id(path: &Path) -> io::Result<u64> {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    let stats = unsafe { stats.assume_init() };
    Ok(stats.f_fsid)
}

fn available_bytes(path: &Path) -> io::Result<u64> {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    let stats = unsafe { stats.assume_init() };
    #[cfg(target_os = "macos")]
    let available_blocks = u64::from(stats.f_bavail);
    #[cfg(not(target_os = "macos"))]
    let available_blocks = stats.f_bavail;
    Ok(available_blocks.saturating_mul(stats.f_frsize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn cleanup_only_removes_recognized_regular_files_and_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        let bin = tmp.path().join("usr/local/bin");
        fs::create_dir_all(&bin).unwrap();
        let backup = bin.join("xp.bak.1");
        let failed = bin.join("xray.failed.2");
        let unknown = bin.join("keep-me");
        let link = bin.join("xp.bak.link");
        fs::write(&backup, b"old").unwrap();
        fs::write(&failed, b"bad").unwrap();
        fs::write(&unknown, b"keep").unwrap();
        symlink(&unknown, &link).unwrap();
        let workspace = workspace_path(&paths);
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("xray.zip"), b"zip").unwrap();

        cleanup_managed_artifacts_for(&paths, &[]).unwrap();

        assert!(!backup.exists());
        assert!(!failed.exists());
        assert!(unknown.exists());
        assert!(link.exists());
        assert!(!workspace.exists());
    }

    #[test]
    fn recognized_artifact_names_are_strict() {
        assert!(is_managed_artifact_name("xp.bak.1", "xp"));
        assert!(is_managed_artifact_name("xp.failed.1", "xp"));
        assert!(is_managed_artifact_name(".xp.tmp.42", "xp"));
        assert!(!is_managed_artifact_name("xp-backup", "xp"));
        assert!(!is_managed_artifact_name("xp.bak.", "xp"));
        assert!(!is_managed_artifact_name("not-xp.bak.1", "xp"));
    }

    #[test]
    fn storage_uses_only_the_selected_cloudflared_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        let local = paths.usr_local_bin_cloudflared();
        fs::create_dir_all(local.parent().unwrap()).unwrap();
        fs::write(&local, b"cloudflared").unwrap();

        let installation = managed_installation_binaries(&paths);
        assert!(installation.contains(&local));
        assert!(!installation.contains(&paths.usr_bin_cloudflared()));
    }

    #[test]
    fn cleanup_rejects_symlinked_workspace_and_managed_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        let workspace = workspace_path(&paths);
        fs::create_dir_all(workspace.parent().unwrap()).unwrap();
        let outside = tmp.path().join("outside-workspace");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, &workspace).unwrap();
        assert!(cleanup_managed_artifacts_for(&paths, &[]).is_err());
        assert!(outside.exists());

        fs::remove_file(&workspace).unwrap();
        let usr = tmp.path().join("usr");
        fs::create_dir_all(&usr).unwrap();
        let outside_local = tmp.path().join("outside-local");
        fs::create_dir(&outside_local).unwrap();
        symlink(&outside_local, usr.join("local")).unwrap();
        assert!(cleanup_managed_artifacts_for(&paths, &[]).is_err());
        assert!(outside_local.exists());
    }
}
