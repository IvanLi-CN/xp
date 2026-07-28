use crate::ops::init::backfill_low_memory_runtime_defaults;
use crate::ops::paths::Paths;
use std::fs;
use std::os::unix::fs::PermissionsExt;
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

#[test]
fn low_memory_backfill_restores_openrc_executable_mode() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("xray");
    fs::write(&service, "command_user=\"xray:xray\"\n").unwrap();
    fs::set_permissions(&service, fs::Permissions::from_mode(0o640)).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    assert_eq!(
        fs::metadata(service).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn low_memory_backfill_supports_provider_wrapper_script() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("cloudflared");
    fs::write(
        &service,
        "#!/sbin/openrc-run\ncommand=/usr/local/libexec/cloudflared-tunnel\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(&service).unwrap();
    assert!(updated.starts_with("#!/sbin/openrc-run\nexport GOMEMLIMIT="));
    assert!(updated.contains("GOMEMLIMIT:-12MiB"));
    assert_eq!(
        fs::metadata(service).unwrap().permissions().mode() & 0o777,
        0o755
    );
}
