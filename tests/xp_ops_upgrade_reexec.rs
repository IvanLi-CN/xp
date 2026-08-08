#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

fn copy_current_xp_ops(dest: &Path) {
    fs::copy(assert_cmd::cargo::cargo_bin("xp-ops"), dest).unwrap();
    let mut permissions = fs::metadata(dest).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(dest, permissions).unwrap();
}

#[test]
fn complete_phase_cleanup_failure_restores_previous_xp_ops() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_string_lossy().to_string();
    let data_dir = tmp.path().join("data").to_string_lossy().to_string();
    let dest = tmp.path().join("xp-ops-copy");
    let backup = tmp.path().join("xp-ops-copy.bak.resume");
    copy_current_xp_ops(&dest);
    fs::write(&backup, b"xp-ops-old").unwrap();

    let workspace = tmp.path().join("tmp/xp-ops");
    fs::create_dir_all(workspace.parent().unwrap()).unwrap();
    symlink(tmp.path().join("workspace-target"), &workspace).unwrap();

    let mut command = assert_cmd::Command::new(&dest);
    command
        .env("XP_OPS_UPGRADE_RESUME_TAG", "v0.1.999")
        .env("XP_OPS_UPGRADE_RESUME_REPO", "o/r")
        .env("XP_OPS_UPGRADE_RESUME_API_BASE", "http://127.0.0.1:1")
        .env("XP_OPS_UPGRADE_RESUME_XP_OPS_DEST", &dest)
        .env("XP_OPS_UPGRADE_RESUME_XP_OPS_BACKUP", &backup)
        .env("XP_OPS_UPGRADE_RESUME_SERVICE_PHASE_COMPLETE", "1")
        .args([
            "--root",
            &root,
            "upgrade",
            "--version",
            "v0.1.999",
            "--repo",
            "o/r",
            "--data-dir",
            &data_dir,
        ]);
    command.assert().failure().code(7);

    assert_eq!(fs::read(&dest).unwrap(), b"xp-ops-old");
    assert!(!backup.exists());
    assert!(fs::read_dir(tmp.path()).unwrap().all(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_str()
            .is_none_or(|name| !name.starts_with("xp-ops-copy.failed."))
    }));
}
