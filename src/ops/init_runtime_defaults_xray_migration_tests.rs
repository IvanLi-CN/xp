use crate::ops::init::backfill_low_memory_runtime_defaults;
use crate::ops::paths::Paths;
use std::fs;
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
    .replace(
        "Environment=GOMEMLIMIT=32MiB\nEnvironment=GOGC=100\n",
        "Environment=GOMEMLIMIT=16MiB\nEnvironment=GOGC=50\n",
    );
    fs::write(systemd.join("xray.service"), legacy).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(systemd.join("xray.service.d/20-xp-memory.conf")).unwrap();
    assert!(managed.contains("Environment=GOMEMLIMIT=32MiB"));
    assert!(managed.contains("Environment=GOGC=100"));

    fs::write(
        systemd.join("xray.service"),
        crate::ops::ingress_guard::render_systemd_xray_unit(
            std::path::Path::new("/var/lib/xray"),
            None,
        ),
    )
    .unwrap();
    backfill_low_memory_runtime_defaults(&paths).unwrap();

    assert_eq!(
        fs::read_to_string(systemd.join("xray.service.d/20-xp-memory.conf")).unwrap(),
        "[Service]\n# Managed by xp-ops; use a separate drop-in for overrides\n"
    );
}

#[test]
fn low_memory_backfill_migrates_documented_systemd_xray_defaults() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    let legacy = include_str!("../../docs/ops/systemd/xray.service")
        .replace(
            "Environment=GOMEMLIMIT=32MiB",
            "Environment=GOMEMLIMIT=16MiB",
        )
        .replace("Environment=GOGC=100", "Environment=GOGC=50");
    fs::write(systemd.join("xray.service"), legacy).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(systemd.join("xray.service.d/20-xp-memory.conf")).unwrap();
    assert!(managed.contains("Environment=GOMEMLIMIT=32MiB"));
    assert!(managed.contains("Environment=GOGC=100"));
}

#[test]
fn low_memory_backfill_preserves_systemd_xray_operator_legacy_value() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    fs::write(
        systemd.join("xray.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=16MiB\nEnvironment=GOGC=50\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(systemd.join("xray.service.d/20-xp-memory.conf")).unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
    assert!(!managed.contains("Environment=GOGC="));
}

#[test]
fn low_memory_backfill_migrates_generated_openrc_xray_defaults() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("xray");
    let legacy = crate::ops::ingress_guard::render_openrc_xray_script(None).replace(
        "export GOMEMLIMIT=\"${GOMEMLIMIT:-32MiB}\"\nexport GOGC=\"${GOGC:-100}\"\n",
        "export GOMEMLIMIT=\"${GOMEMLIMIT:-16MiB}\"\nexport GOGC=\"${GOGC:-50}\"\n",
    );
    fs::write(&service, legacy).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(service).unwrap();
    assert!(updated.contains("GOMEMLIMIT:-32MiB"));
    assert!(updated.contains("GOGC:-100"));
}

#[test]
fn low_memory_backfill_preserves_openrc_xray_operator_legacy_value() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("xray");
    let original = concat!(
        "command_user=\"xray:xray\"\n",
        "export GOMEMLIMIT=\"${GOMEMLIMIT:-16MiB}\"\n",
        "export GOGC=\"${GOGC:-50}\"\n",
    );
    fs::write(&service, original).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(service).unwrap();
    assert!(updated.contains("GOMEMLIMIT:-16MiB"));
    assert!(updated.contains("GOGC:-50"));
}
