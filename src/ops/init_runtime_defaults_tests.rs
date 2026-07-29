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
    assert!(cloudflared.contains("GOMEMLIMIT=8MiB"));
    assert!(cloudflared.contains("TUNNEL_MANAGEMENT_DIAGNOSTICS=false"));
    assert!(cloudflared.contains("XP_CLOUDFLARED_PROTOCOL=http2"));
}

#[test]
fn low_memory_backfill_preserves_systemd_operator_overrides() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=20MiB\nEnvironment=GOGC=75\n",
    )
    .unwrap();
    fs::write(
        systemd.join("cloudflared.service.d/10-operator.conf"),
        "[Service]\nEnvironment=TUNNEL_MANAGEMENT_DIAGNOSTICS=true\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert_eq!(
        managed,
        concat!(
            "[Service]\n",
            "# Managed by xp-ops; use a separate drop-in for overrides\n",
            "Environment=XP_CLOUDFLARED_PROTOCOL=http2\n",
        )
    );
}

#[test]
fn low_memory_backfill_migrates_legacy_systemd_default() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=\"GOMEMLIMIT=12MiB\" \"GOGC=50\"\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(managed.contains("GOMEMLIMIT=8MiB"));
    assert!(!managed.contains("Environment=GOGC=50"));
    assert!(managed.contains("TUNNEL_MANAGEMENT_DIAGNOSTICS=false"));
}

#[test]
fn low_memory_backfill_preserves_customized_managed_systemd_drop_in() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    let drop_in = systemd.join("cloudflared.service.d/20-xp-memory.conf");
    fs::create_dir_all(drop_in.parent().unwrap()).unwrap();
    let unit = systemd.join("cloudflared.service");
    let original_unit = concat!(
        "[Service]\n",
        "ExecStart=/usr/bin/cloudflared --no-autoupdate ",
        "--config /etc/cloudflared/config.yml tunnel run\n",
    );
    fs::write(&unit, original_unit).unwrap();
    let custom = "[Service]\nEnvironment=GOMEMLIMIT=20MiB\nEnvironment=GOGC=75\n";
    fs::write(&drop_in, custom).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    assert_eq!(fs::read_to_string(drop_in).unwrap(), custom);
    let updated_unit = fs::read_to_string(unit).unwrap();
    assert!(updated_unit.contains("Environment=XP_CLOUDFLARED_PROTOCOL=http2"));
    assert!(updated_unit.contains("--protocol ${XP_CLOUDFLARED_PROTOCOL} --config"));
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
fn low_memory_backfill_migrates_legacy_openrc_default() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("cloudflared");
    fs::write(
        &service,
        concat!(
            "command_user=\"cloudflared:cloudflared\"\n",
            "command_args=\"--no-autoupdate --config /etc/cloudflared/config.yml tunnel run\"\n",
            "export GOMEMLIMIT=\"${GOMEMLIMIT:-12MiB}\"\n",
            "export GOGC=\"${GOGC:-50}\"\n",
        ),
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(service).unwrap();
    assert!(updated.contains("GOMEMLIMIT:-8MiB"));
    assert!(!updated.contains("GOMEMLIMIT:-12MiB"));
    assert!(updated.contains("TUNNEL_MANAGEMENT_DIAGNOSTICS:-false"));
    assert!(updated.contains(concat!(
        "--protocol ${XP_CLOUDFLARED_PROTOCOL:-http2}",
        " --config /etc/cloudflared/config.yml",
    )));
}

#[test]
fn low_memory_backfill_adds_http2_without_replacing_explicit_protocols() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    let unit = systemd.join("cloudflared.service");
    fs::write(
        &unit,
        concat!(
            "[Service]\n",
            "Environment=XP_CLOUDFLARED_PROTOCOL=quic\n",
            "ExecStart=/usr/bin/cloudflared --no-autoupdate ",
            "--config /etc/cloudflared/config.yml tunnel run\n",
        ),
    )
    .unwrap();
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let openrc = paths.openrc_initd_dir().join("cloudflared");
    fs::write(
        &openrc,
        concat!(
            "command_args=\"--no-autoupdate --protocol quic ",
            "--config /etc/cloudflared/config.yml tunnel run\"\n",
        ),
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    assert!(
        fs::read_to_string(&unit)
            .unwrap()
            .contains("--protocol ${XP_CLOUDFLARED_PROTOCOL} --config")
    );
    assert!(
        fs::read_to_string(&unit)
            .unwrap()
            .contains("Environment=XP_CLOUDFLARED_PROTOCOL=quic")
    );
    assert!(
        fs::read_to_string(openrc)
            .unwrap()
            .contains("--protocol quic --config")
    );
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
    assert!(updated.contains("GOMEMLIMIT:-8MiB"));
    assert!(updated.contains("TUNNEL_MANAGEMENT_DIAGNOSTICS:-false"));
    assert_eq!(
        fs::metadata(service).unwrap().permissions().mode() & 0o777,
        0o755
    );
}
