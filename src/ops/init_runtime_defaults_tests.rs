use crate::ops::init::backfill_low_memory_runtime_defaults;
use crate::ops::paths::Paths;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
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
fn low_memory_backfill_preserves_runtime_systemd_drop_in_override() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    let runtime_drop_in = paths
        .map_abs(std::path::Path::new("/run/systemd/system"))
        .join("cloudflared.service.d/30-operator.conf");
    fs::create_dir_all(runtime_drop_in.parent().unwrap()).unwrap();
    fs::write(runtime_drop_in, "[Service]\nEnvironment=GOMEMLIMIT=24MiB\n").unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_applies_systemd_drop_in_directory_masking() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    let vendor_drop_in = paths
        .map_abs(std::path::Path::new("/usr/lib/systemd/system"))
        .join("cloudflared.service.d/10-memory.conf");
    fs::create_dir_all(vendor_drop_in.parent().unwrap()).unwrap();
    fs::write(vendor_drop_in, "[Service]\nEnvironment=GOMEMLIMIT=24MiB\n").unwrap();
    fs::write(
        systemd.join("cloudflared.service.d/10-memory.conf"),
        "[Service]\nEnvironment=GOMEMLIMIT=32MiB\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_does_not_mask_lower_priority_managed_name() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    fs::write(systemd.join("xray.service"), "[Service]\n").unwrap();
    let vendor_drop_in = paths
        .map_abs(std::path::Path::new("/usr/lib/systemd/system"))
        .join("xray.service.d/20-xp-memory.conf");
    fs::create_dir_all(vendor_drop_in.parent().unwrap()).unwrap();
    fs::write(&vendor_drop_in, "[Service]\nEnvironment=GOMEMLIMIT=24MiB\n").unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    assert!(!systemd.join("xray.service.d/20-xp-memory.conf").exists());
    assert!(
        fs::read_to_string(vendor_drop_in)
            .unwrap()
            .contains("GOMEMLIMIT=24MiB")
    );
}

#[test]
fn low_memory_backfill_preserves_legacy_value_in_operator_drop_in() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::write(systemd.join("cloudflared.service"), "[Service]\n").unwrap();
    fs::write(
        systemd.join("cloudflared.service.d/10-operator.conf"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(!managed.contains("GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_preserves_systemd_operator_unset() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    fs::write(
        systemd.join("cloudflared.service.d/10-operator.conf"),
        "[Service]\nEnvironment=GOMEMLIMIT\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_preserves_systemd_operator_environment_reset() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        concat!(
            "[Unit]\nDescription=cloudflared (Cloudflare Tunnel)\n",
            "Wants=network-online.target\nAfter=network-online.target\n\n",
            "[Service]\nType=simple\nUser=cloudflared\nGroup=cloudflared\n",
            "Environment=GOMEMLIMIT=8MiB\nEnvironment=GOGC=50\n",
            "Environment=TUNNEL_MANAGEMENT_DIAGNOSTICS=false\n",
            "ExecStart=/usr/bin/cloudflared --no-autoupdate ",
            "--config /etc/cloudflared/config.yml tunnel run\n",
            "Restart=always\nRestartSec=2s\n\n",
            "[Install]\nWantedBy=multi-user.target\n",
        ),
    )
    .unwrap();
    fs::write(
        systemd.join("cloudflared.service.d/10-operator.conf"),
        "[Service]\nEnvironment=\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert_eq!(
        managed,
        "[Service]\n# Managed by xp-ops; use a separate drop-in for overrides\n"
    );
}

#[test]
fn low_memory_backfill_accepts_non_regular_systemd_environment_file() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    fs::write(
        systemd.join("cloudflared.service.d/10-operator.conf"),
        "[Service]\nEnvironmentFile=/etc/cloudflared/empty.env\n",
    )
    .unwrap();
    symlink("/dev/null", paths.etc_cloudflared_dir().join("empty.env")).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_migrates_legacy_systemd_default() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        concat!(
            "[Unit]\n",
            "Description=cloudflared (Cloudflare Tunnel)\n",
            "Wants=network-online.target\n",
            "After=network-online.target\n\n",
            "[Service]\n",
            "Type=simple\n",
            "User=cloudflared\n",
            "Group=cloudflared\n",
            "Environment=GOMEMLIMIT=8MiB\n",
            "Environment=GOGC=50\n",
            "Environment=TUNNEL_MANAGEMENT_DIAGNOSTICS=false\n",
            "ExecStart=/usr/bin/cloudflared --no-autoupdate ",
            "--config /etc/cloudflared/config.yml tunnel run\n",
            "Restart=always\n",
            "RestartSec=2s\n\n",
            "[Install]\n",
            "WantedBy=multi-user.target\n",
        ),
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(managed.contains("GOMEMLIMIT=12MiB"));
    assert!(!managed.contains("Environment=GOGC=50"));
    assert!(!managed.contains("TUNNEL_MANAGEMENT_DIAGNOSTICS=false"));
}

#[test]
fn low_memory_backfill_preserves_operator_owned_systemd_legacy_value() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_preserves_operator_owned_openrc_legacy_value() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("cloudflared");
    let original = concat!(
        "#!/sbin/openrc-run\n",
        "command_user=\"cloudflared:cloudflared\"\n",
        "export GOMEMLIMIT=\"${GOMEMLIMIT:-8MiB}\"\n",
        "export GOGC=\"${GOGC:-50}\"\n",
    );
    fs::write(&service, original).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(service).unwrap();
    assert!(updated.contains("GOMEMLIMIT:-8MiB"));
    assert!(!updated.contains("GOMEMLIMIT:-12MiB"));
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
fn low_memory_backfill_ignores_absent_optional_systemd_environment_file() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    fs::write(
        systemd
            .join("cloudflared.service.d")
            .join("10-operator.conf"),
        "[Service]\nEnvironmentFile=-/etc/cloudflared/operator.env\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(
        systemd
            .join("cloudflared.service.d")
            .join("20-xp-memory.conf"),
    )
    .unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_preserves_systemd_environment_file_override() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    fs::write(
        systemd
            .join("cloudflared.service.d")
            .join("10-operator.conf"),
        "[Service]\nEnvironmentFile=/etc/cloudflared/operator.env\n",
    )
    .unwrap();
    fs::write(
        paths.etc_cloudflared_dir().join("operator.env"),
        "GOMEMLIMIT=24MiB\nUNRELATED=value\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(
        systemd
            .join("cloudflared.service.d")
            .join("20-xp-memory.conf"),
    )
    .unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
    assert!(managed.contains("Environment=GOGC=50"));
}

#[test]
fn low_memory_backfill_parses_continued_systemd_environment_file_directive() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    fs::write(
        systemd.join("cloudflared.service.d/10-operator.conf"),
        concat!(
            "[Service]\n",
            "EnvironmentFile=-/etc/cloudflared/absent.env \\\n",
            "  /etc/cloudflared/operator.env\n",
        ),
    )
    .unwrap();
    fs::write(
        paths.etc_cloudflared_dir().join("operator.env"),
        "GOMEMLIMIT=24MiB\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_expands_systemd_environment_file_wildcards() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    fs::write(
        systemd
            .join("cloudflared.service.d")
            .join("10-operator.conf"),
        "[Service]\nEnvironmentFile=/etc/cloudflared/*.env\n",
    )
    .unwrap();
    fs::write(
        paths.etc_cloudflared_dir().join("memory.env"),
        "GOMEMLIMIT=24MiB\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(
        systemd
            .join("cloudflared.service.d")
            .join("20-xp-memory.conf"),
    )
    .unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_reads_base_unit_environment_file_with_specifiers() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\nEnvironmentFile=/etc/cloudflared/%N.env\n",
    )
    .unwrap();
    fs::write(
        paths.etc_cloudflared_dir().join("cloudflared.env"),
        "GOMEMLIMIT=24MiB\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(
        systemd
            .join("cloudflared.service.d")
            .join("20-xp-memory.conf"),
    )
    .unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_normalizes_environment_file_parent_components() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(&systemd).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        concat!(
            "[Service]\n",
            "Environment=GOMEMLIMIT=8MiB\n",
            "EnvironmentFile=/etc/cloudflared/../xp/runtime.env\n",
        ),
    )
    .unwrap();
    let environment_file = paths.map_abs(std::path::Path::new("/etc/xp/runtime.env"));
    fs::create_dir_all(environment_file.parent().unwrap()).unwrap();
    fs::write(environment_file, "GOMEMLIMIT=24MiB\n").unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed =
        fs::read_to_string(systemd.join("cloudflared.service.d/20-xp-memory.conf")).unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_ignores_semicolon_comments_in_environment_files() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    fs::write(
        systemd
            .join("cloudflared.service.d")
            .join("10-operator.conf"),
        "[Service]\nEnvironmentFile=/etc/cloudflared/operator.env\n",
    )
    .unwrap();
    fs::write(
        paths.etc_cloudflared_dir().join("operator.env"),
        "; GOMEMLIMIT=24MiB\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(
        systemd
            .join("cloudflared.service.d")
            .join("20-xp-memory.conf"),
    )
    .unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_honors_environment_file_reset() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        concat!(
            "[Service]\n",
            "Environment=GOMEMLIMIT=8MiB\n",
            "EnvironmentFile=/etc/cloudflared/stale.env\n",
        ),
    )
    .unwrap();
    fs::write(
        systemd.join("cloudflared.service.d").join("10-reset.conf"),
        "[Service]\nEnvironmentFile=\n",
    )
    .unwrap();
    fs::write(
        paths.etc_cloudflared_dir().join("stale.env"),
        "GOMEMLIMIT=24MiB\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(
        systemd
            .join("cloudflared.service.d")
            .join("20-xp-memory.conf"),
    )
    .unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_preserves_unknown_environment_file_specifiers() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    fs::write(
        systemd
            .join("cloudflared.service.d")
            .join("10-operator.conf"),
        "[Service]\nEnvironmentFile=-/etc/cloudflared/%H.env\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(
        systemd
            .join("cloudflared.service.d")
            .join("20-xp-memory.conf"),
    )
    .unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
}

#[test]
fn low_memory_backfill_globs_do_not_match_hidden_environment_files() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let systemd = paths.systemd_unit_dir();
    fs::create_dir_all(systemd.join("cloudflared.service.d")).unwrap();
    fs::create_dir_all(paths.etc_cloudflared_dir()).unwrap();
    fs::write(
        systemd.join("cloudflared.service"),
        "[Service]\nEnvironment=GOMEMLIMIT=8MiB\n",
    )
    .unwrap();
    fs::write(
        systemd
            .join("cloudflared.service.d")
            .join("10-operator.conf"),
        "[Service]\nEnvironmentFile=-/etc/cloudflared/*.env\n",
    )
    .unwrap();
    fs::write(
        paths.etc_cloudflared_dir().join(".backup.env"),
        "GOMEMLIMIT=24MiB\n",
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let managed = fs::read_to_string(
        systemd
            .join("cloudflared.service.d")
            .join("20-xp-memory.conf"),
    )
    .unwrap();
    assert!(!managed.contains("Environment=GOMEMLIMIT="));
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
            "#!/sbin/openrc-run\n\n",
            "name=\"cloudflared\"\n",
            "description=\"cloudflared (Cloudflare Tunnel)\"\n\n",
            "command=\"/usr/local/bin/cloudflared\"\n",
            "command_args=\"--no-autoupdate --config /etc/cloudflared/config.yml tunnel run\"\n",
            "command_user=\"cloudflared:cloudflared\"\n",
            "export GOMEMLIMIT=\"${GOMEMLIMIT:-8MiB}\"\n",
            "export GOGC=\"${GOGC:-50}\"\n",
            "export TUNNEL_MANAGEMENT_DIAGNOSTICS=\"",
            "${TUNNEL_MANAGEMENT_DIAGNOSTICS:-false}\"\n\n",
            "# Ensure automatic recovery on crashes without busy-looping.\n",
            "supervisor=supervise-daemon\n",
            "respawn_delay=2\n",
            "respawn_max=0\n\n",
            "depend() {\n",
            "  need net\n",
            "}\n",
        ),
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(service).unwrap();
    assert!(updated.contains("GOMEMLIMIT:-12MiB"));
    assert!(!updated.contains("GOMEMLIMIT:-8MiB"));
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
    fs::create_dir_all(paths.usr_local_libexec_dir()).unwrap();
    let wrapper = paths.usr_local_libexec_dir().join("cloudflared-tunnel");
    fs::write(
        &wrapper,
        concat!(
            "#!/bin/sh\n",
            "exec /usr/local/bin/cloudflared tunnel --no-autoupdate run ",
            "--token \"$(cat /etc/cloudflared/tunnel-token)\"\n",
        ),
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(&service).unwrap();
    assert!(updated.starts_with("#!/sbin/openrc-run\nexport GOMEMLIMIT="));
    assert!(updated.contains("GOMEMLIMIT:-12MiB"));
    assert!(updated.contains("TUNNEL_MANAGEMENT_DIAGNOSTICS:-false"));
    assert!(updated.contains("XP_CLOUDFLARED_PROTOCOL:-http2"));
    assert_eq!(
        fs::read_to_string(&wrapper).unwrap(),
        concat!(
            "#!/bin/sh\n",
            "exec /usr/local/bin/cloudflared --no-autoupdate ",
            "--protocol \"${XP_CLOUDFLARED_PROTOCOL:-http2}\" ",
            "tunnel run --token-file /etc/cloudflared/tunnel-token\n",
        )
    );
    assert_eq!(
        fs::metadata(wrapper).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        fs::metadata(service).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn low_memory_backfill_preserves_ambiguous_provider_wrapper_memory_default() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.openrc_initd_dir()).unwrap();
    let service = paths.openrc_initd_dir().join("cloudflared");
    fs::write(
        &service,
        concat!(
            "#!/sbin/openrc-run\n",
            "command=/usr/local/libexec/cloudflared-tunnel\n",
            "command_user=\"cloudflared:cloudflared\"\n",
            "export GOMEMLIMIT=\"${GOMEMLIMIT:-8MiB}\"\n",
            "# operator-owned settings\n",
        ),
    )
    .unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    let updated = fs::read_to_string(service).unwrap();
    assert!(updated.contains("GOMEMLIMIT:-8MiB"));
    assert!(!updated.contains("GOMEMLIMIT:-12MiB"));
}

#[test]
fn low_memory_backfill_preserves_custom_provider_wrapper_script() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.usr_local_libexec_dir()).unwrap();
    let wrapper = paths.usr_local_libexec_dir().join("cloudflared-tunnel");
    let custom = "#!/bin/sh\nexec /usr/local/bin/cloudflared --protocol quic tunnel run\n";
    fs::write(&wrapper, custom).unwrap();

    backfill_low_memory_runtime_defaults(&paths).unwrap();

    assert_eq!(fs::read_to_string(wrapper).unwrap(), custom);
}
