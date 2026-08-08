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
    let data_dir = tmp.path().join("data");
    let data_dir_arg = data_dir.to_string_lossy().to_string();
    let dest = tmp.path().join("xp-ops-copy");
    let backup = tmp.path().join("xp-ops-copy.bak.resume");
    copy_current_xp_ops(&dest);
    fs::write(&backup, b"xp-ops-old").unwrap();
    let xp = tmp.path().join("usr/local/bin/xp");
    let xp_backup = tmp.path().join("usr/local/bin/xp.bak.resume");
    fs::create_dir_all(xp.parent().unwrap()).unwrap();
    fs::write(&xp, b"xp-new").unwrap();
    fs::write(&xp_backup, b"xp-old").unwrap();
    let service_backups = serde_json::json!([{
        "dest": xp,
        "backup": xp_backup,
    }])
    .to_string();

    let workspace = tmp.path().join("tmp/xp-ops");
    fs::create_dir_all(workspace.parent().unwrap()).unwrap();
    symlink(tmp.path().join("workspace-target"), &workspace).unwrap();
    let upgrade_dir = data_dir.join("upgrade");
    fs::create_dir_all(&upgrade_dir).unwrap();
    fs::write(
        upgrade_dir.join("request.json"),
        serde_json::json!({
            "target_tag": "v0.1.999",
            "repo": "o/r",
            "requested_at": "2026-07-04T00:00:00Z"
        })
        .to_string(),
    )
    .unwrap();

    let mut command = assert_cmd::Command::new(&dest);
    command
        .env("XP_OPS_UPGRADE_RESUME_TAG", "v0.1.999")
        .env("XP_OPS_UPGRADE_RESUME_REPO", "o/r")
        .env("XP_OPS_UPGRADE_RESUME_API_BASE", "http://127.0.0.1:1")
        .env("XP_OPS_UPGRADE_RESUME_XP_OPS_DEST", &dest)
        .env("XP_OPS_UPGRADE_RESUME_XP_OPS_BACKUP", &backup)
        .env("XP_OPS_UPGRADE_RESUME_SERVICE_BACKUPS", service_backups)
        .env("XP_OPS_UPGRADE_RESUME_SERVICE_PHASE_COMPLETE", "1")
        .args([
            "--root",
            &root,
            "_upgrade-runner",
            "--data-dir",
            &data_dir_arg,
        ]);
    command.assert().failure().code(7);

    assert_eq!(fs::read(&dest).unwrap(), b"xp-ops-old");
    assert_eq!(fs::read(&xp).unwrap(), b"xp-old");
    assert!(!backup.exists());
    assert!(!xp_backup.exists());
    assert!(fs::read_dir(tmp.path()).unwrap().all(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_str()
            .is_none_or(|name| !name.starts_with("xp-ops-copy.failed."))
    }));
    let status: serde_json::Value =
        serde_json::from_slice(&fs::read(data_dir.join("upgrade/status.json")).unwrap()).unwrap();
    assert_eq!(status["state"], "failed");
}

#[test]
fn complete_phase_status_preflight_failure_restores_all_binaries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_string_lossy().to_string();
    let data_dir = tmp.path().join("data");
    let data_dir_arg = data_dir.to_string_lossy().to_string();
    let dest = tmp.path().join("xp-ops-copy");
    let backup = tmp.path().join("xp-ops-copy.bak.resume");
    copy_current_xp_ops(&dest);
    fs::write(&backup, b"xp-ops-old").unwrap();
    let xp = tmp.path().join("usr/local/bin/xp");
    let xp_backup = tmp.path().join("usr/local/bin/xp.bak.resume");
    fs::create_dir_all(xp.parent().unwrap()).unwrap();
    fs::write(&xp, b"xp-new").unwrap();
    fs::write(&xp_backup, b"xp-old").unwrap();
    let service_backups = serde_json::json!([{
        "dest": xp,
        "backup": xp_backup,
    }])
    .to_string();
    let upgrade_dir = data_dir.join("upgrade");
    fs::create_dir_all(upgrade_dir.join("status.json.tmp")).unwrap();
    fs::write(
        upgrade_dir.join("request.json"),
        serde_json::json!({
            "target_tag": "v0.1.999",
            "repo": "o/r",
            "requested_at": "2026-07-04T00:00:00Z"
        })
        .to_string(),
    )
    .unwrap();

    let mut command = assert_cmd::Command::new(&dest);
    command
        .env("XP_OPS_UPGRADE_RESUME_TAG", "v0.1.999")
        .env("XP_OPS_UPGRADE_RESUME_REPO", "o/r")
        .env("XP_OPS_UPGRADE_RESUME_API_BASE", "http://127.0.0.1:1")
        .env("XP_OPS_UPGRADE_RESUME_XP_OPS_DEST", &dest)
        .env("XP_OPS_UPGRADE_RESUME_XP_OPS_BACKUP", &backup)
        .env("XP_OPS_UPGRADE_RESUME_SERVICE_BACKUPS", service_backups)
        .env("XP_OPS_UPGRADE_RESUME_SERVICE_PHASE_COMPLETE", "1")
        .args([
            "--root",
            &root,
            "_upgrade-runner",
            "--data-dir",
            &data_dir_arg,
        ]);
    command.assert().failure().code(7);

    assert_eq!(fs::read(&dest).unwrap(), b"xp-ops-old");
    assert_eq!(fs::read(&xp).unwrap(), b"xp-old");
    assert!(!backup.exists());
    assert!(!xp_backup.exists());
}
