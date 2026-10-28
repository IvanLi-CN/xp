use crate::ops::init::backfill_low_memory_runtime_defaults;
use crate::ops::paths::Paths;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

#[test]
fn low_memory_backfill_migrates_generated_systemd_xray_defaults() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    let legacy = crate::ops::ingress_guard::render_systemd_xray_unit(
        std::path::Path::new("/var/lib/xray"),
        None,
    )
    .replace("GOMEMLIMIT=48MiB", "GOMEMLIMIT=16MiB")
    .replace("GOGC=100", "GOGC=50");
    fs::write(systemd.join("xray.service"), legacy).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(systemd.join("xray.service.d/20-xp-memory.conf")).unwrap();
    assert!(managed.contains("Environment=GOMEMLIMIT=48MiB"));
    assert!(managed.contains("Environment=GOGC=100"));
    assert!(!managed.contains("Environment=GOMEMLIMIT=16MiB"));
    assert!(!managed.contains("Environment=GOGC=50"));
}

#[test]
fn low_memory_backfill_migrates_previous_generated_systemd_xray_defaults() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    let previous = crate::ops::ingress_guard::render_systemd_xray_unit(
        std::path::Path::new("/var/lib/xray"),
        None,
    )
    .replace("GOMEMLIMIT=48MiB", "GOMEMLIMIT=32MiB");
    fs::write(systemd.join("xray.service"), previous).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(systemd.join("xray.service.d/20-xp-memory.conf")).unwrap();
    assert!(managed.contains("Environment=GOMEMLIMIT=48MiB"));
}

#[test]
fn low_memory_backfill_migrates_generated_openrc_xray_defaults() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("xray");
    let legacy = crate::ops::ingress_guard::render_openrc_xray_script(None)
        .replace("${GOMEMLIMIT:-48MiB}", "${GOMEMLIMIT:-16MiB}")
        .replace("${GOGC:-100}", "${GOGC:-50}");
    fs::write(&service, legacy).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(service).unwrap();
    assert!(updated.contains("GOMEMLIMIT:-48MiB"));
    assert!(updated.contains("GOGC:-100"));
    assert!(!updated.contains("GOMEMLIMIT:-16MiB"));
    assert!(!updated.contains("GOGC:-50"));
}

#[test]
fn low_memory_backfill_migrates_previous_generated_openrc_xray_defaults() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("xray");
    let previous = crate::ops::ingress_guard::render_openrc_xray_script(None)
        .replace("${GOMEMLIMIT:-48MiB}", "${GOMEMLIMIT:-32MiB}");
    fs::write(&service, previous).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(service).unwrap();
    assert!(updated.contains("GOMEMLIMIT:-48MiB"));
    assert!(!updated.contains("GOMEMLIMIT:-32MiB"));
}

#[test]
fn low_memory_backfill_restores_openrc_executable_mode() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("xray");
    fs::write(&service, "command_user=\"xray:xray\"\n").unwrap();
    fs::set_permissions(&service, fs::Permissions::from_mode(0o640)).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(&service).unwrap();
    assert!(updated.contains("GOMEMLIMIT:-48MiB"));
    assert!(updated.contains("GOGC:-100"));
    assert_eq!(
        fs::metadata(service).unwrap().permissions().mode() & 0o777,
        0o755
    );
}
