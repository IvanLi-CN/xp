use crate::ops::init::backfill_low_memory_runtime_defaults;
use crate::ops::paths::Paths;
use std::fs;
use tempfile::tempdir;

#[test]
fn low_memory_backfill_writes_systemd_drop_ins() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.systemd_unit_dir()).unwrap();
    fs::write(paths.systemd_unit_dir().join("xray.service"), "[Service]\n").unwrap();
    fs::write(
        paths.systemd_unit_dir().join("cloudflared.service"),
        "[Service]\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let xray = fs::read_to_string(
        paths
            .systemd_unit_dir()
            .join("xray.service.d/20-xp-memory.conf"),
    )
    .unwrap();
    let cloudflared = fs::read_to_string(
        paths
            .systemd_unit_dir()
            .join("cloudflared.service.d/20-xp-memory.conf"),
    )
    .unwrap();
    assert!(xray.contains("GOMEMLIMIT=16MiB"));
    assert!(cloudflared.contains("GOMEMLIMIT=12MiB"));
}

#[test]
fn low_memory_backfill_preserves_openrc_override() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("xray");
    let original = "command_user=\"xray:xray\"\nexport GOMEMLIMIT=\"20MiB\"\nexport GOGC=\"75\"\n";
    fs::write(&service, original).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    assert_eq!(fs::read_to_string(service).unwrap(), original);
}
