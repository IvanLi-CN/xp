use super::*;
use crate::ops::paths::Paths;
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn ledger(directory: &Path, payload: &[u8]) {
    fs::create_dir_all(directory).unwrap();
    let connection = Connection::open(directory.join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE history_snapshots(key TEXT PRIMARY KEY, payload BLOB NOT NULL);",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO history_snapshots VALUES ('repository_replica', ?1)",
            [payload],
        )
        .unwrap();
}

fn executable(path: &Path, version: &str) {
    fs::write(path, format!("#!/bin/sh\nprintf '%s\\n' 'xp {version}'\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn prefix_history_compatibility_rejects_legacy_targets_without_changing_evidence() {
    let root = tempfile::tempdir().unwrap();
    let payload = br#"{"prefix_recoveries":[{"retained_records":[{"proof":"retained"}]}]}"#;
    ledger(root.path(), payload);
    for tag in ["v3.47.8", "3.47.6", "v3.47.9-rc.1", "unknown"] {
        assert!(ensure_target(root.path(), tag).is_err(), "{tag}");
    }
    for tag in [
        "v3.47.9",
        "3.47.9+build",
        "3.47.9+build-1",
        "v3.48.0-rc.1",
        "4.0.0",
    ] {
        ensure_target(root.path(), tag).unwrap();
    }
    let connection = Connection::open(root.path().join("history.sqlite3")).unwrap();
    let retained: Vec<u8> = connection
        .query_row("SELECT payload FROM history_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(retained, payload);
}

#[test]
fn prefix_history_compatibility_preserves_legacy_behavior_without_recovery() {
    let root = tempfile::tempdir().unwrap();
    ensure_target(root.path(), "v3.47.8").unwrap();
    assert!(!root.path().join("history.sqlite3").exists());
    ledger(root.path(), br#"{"prefix_recoveries":[]}"#);
    ensure_target(root.path(), "v3.47.8").unwrap();
    ensure_rollback(root.path(), &root.path().join("absent-backup")).unwrap();
}

#[test]
fn prefix_history_compatibility_fails_closed_on_unreadable_ledger() {
    for payload in [b"not-json".as_slice(), br#"{"prefix_recoveries":{}}"#] {
        let root = tempfile::tempdir().unwrap();
        ledger(root.path(), payload);
        assert!(ensure_target(root.path(), "v3.47.8").is_err());
        let error = ensure_rollback(root.path(), Path::new("/unused")).unwrap_err();
        assert!(error.message.contains(ROLLBACK_BLOCKED));
        assert!(error.message.contains("transaction_backup_unrestored"));
    }
}

#[test]
fn prefix_history_compatibility_checks_actual_backup_before_any_complete_rollback() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().to_path_buf());
    let data = root.path().join("data");
    ledger(&data, br#"{"prefix_recoveries":[{}]}"#);
    let destination = paths.usr_local_bin_xp();
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    executable(&destination, "3.47.9");
    let backup = destination.with_extension("bak.test");
    executable(&backup, "3.47.8");
    let original = fs::read(&destination).unwrap();
    let backups = [super::super::managed_runtimes::RuntimeBinaryBackup {
        dest: destination.clone(),
        backup: Some(backup.clone()),
    }];
    let error =
        super::super::managed_runtimes::rollback_complete_phase_binaries(&paths, &data, &backups)
            .unwrap_err();
    assert!(error.message.contains(ROLLBACK_BLOCKED));
    assert_eq!(fs::read(&destination).unwrap(), original);
    assert!(backup.exists());
    executable(&backup, "3.47.9");
    ensure_rollback(&data, &backup).unwrap();
    fs::write(&backup, b"not executable XP").unwrap();
    assert!(ensure_rollback(&data, &backup).is_err());
}

#[test]
fn prefix_history_compatibility_keeps_new_ops_and_transaction_backups_on_blocked_rollback() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("xp-ops");
    let backup = root.path().join("xp-ops.bak.test");
    fs::write(&destination, b"new-ops").unwrap();
    fs::write(&backup, b"legacy-ops").unwrap();
    let original = unrestored_transaction_backup_error(ROLLBACK_BLOCKED);
    let error = super::super::failure::rollback_xp_ops_after_resumed_failure(
        &destination,
        &backup,
        original,
    )
    .unwrap_err();
    assert!(error.message.contains(ROLLBACK_BLOCKED));
    assert_eq!(fs::read(destination).unwrap(), b"new-ops");
    assert_eq!(fs::read(backup).unwrap(), b"legacy-ops");
}
