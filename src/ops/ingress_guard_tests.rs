use super::*;
use crate::ops::paths::Paths;
use crate::ops::util::ensure_dir;
use clap::Parser;
use std::fs;
use tempfile::tempdir;

#[test]
fn cli_contract() {
    let mut cli = <crate::ops::cli::Cli as clap::CommandFactory>::command();
    let rendered = cli.render_help().to_string();
    assert!(rendered.contains("ingress-guard"));
    let parsed = crate::ops::cli::Cli::try_parse_from([
        "xp-ops",
        "ingress-guard",
        "enable",
        "--profile",
        "small-vps",
        "--yes",
    ])
    .unwrap();
    assert!(matches!(
        parsed.command,
        Some(crate::ops::cli::Command::IngressGuard(
            crate::ops::cli::IngressGuardCommand::Enable(_)
        ))
    ));
}

#[test]
fn nft_contract() {
    let config = GuardConfig {
        schema: SCHEMA,
        ownership: OWNERSHIP_MARKER.to_string(),
        mode: GuardMode::Enforced,
        profile: "small-vps".to_string(),
        global_rate: 8,
        global_burst: 20,
        source_rate: 3,
        source_burst: 8,
        cgroup: "system.slice/xray.service".to_string(),
    };
    let nft = nft::render(&config).unwrap();
    assert!(nft.contains("limit rate over 8/second burst 20"));
    assert!(nft.contains("limit rate over 3/second burst 8"));
    assert!(nft.contains("meter source_v4 size 1024"));
    assert!(nft.contains("ip saddr timeout 60s"));
    assert!(nft.contains("} counter name source_v4_over_limit drop"));
    assert!(nft.contains("tcp flags & (syn | ack | rst) == syn"));
    assert!(nft.contains("iifname != \"lo\""));
    assert!(nft.contains("counter global_over_limit {}"));
    assert!(nft.contains("counter name global_over_limit drop"));
}

#[test]
fn transaction_contract() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    ensure_dir(&paths.etc_xp_ops_dir()).unwrap();
    let target = paths.etc_xp_ops_ingress_guard_config();
    let config = GuardConfig {
        schema: SCHEMA,
        ownership: OWNERSHIP_MARKER.to_string(),
        mode: GuardMode::Enforced,
        profile: "small-vps".to_string(),
        global_rate: 8,
        global_burst: 20,
        source_rate: 3,
        source_burst: 8,
        cgroup: "system.slice/xray.service".to_string(),
    };
    write_config(&paths, &config).unwrap();
    let permit = permit_token(&config);
    assert_eq!(
        parse_permit_cgroup(&permit).as_deref(),
        Some(config.cgroup.as_str())
    );
    let custom_permit = permit_token(&GuardConfig {
        profile: "custom".to_string(),
        ..config.clone()
    });
    assert_eq!(
        parse_permit_cgroup(&custom_permit).as_deref(),
        Some(config.cgroup.as_str())
    );
    assert!(parse_permit_cgroup("xp-ingress-guard-permit-v1\nschema=2\ncgroup=x\n").is_none());
    let link = target.with_extension("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(unix)]
    assert!(reject_symlink(&link).is_err());
    assert!(fs::read_to_string(target).unwrap().contains("global_rate"));
}

#[test]
fn service_lifecycle_contract() {
    let enforced = render_openrc_xray_script(Some(GuardMode::Enforced));
    assert!(enforced.contains("return \"$result\""));
    let observe = render_openrc_xray_script(Some(GuardMode::Observe));
    assert!(observe.contains("Observe mode records failures"));
    assert!(observe.contains("return 0"));

    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    let service = paths.systemd_unit_dir().join("xray.service");
    ensure_dir(service.parent().unwrap()).unwrap();
    fs::write(
        &service,
        render_systemd_xray_unit(std::path::Path::new("/var/lib/xray"), None)
            .replace("# Managed by xp-ops ingress-guard service boundary\n", ""),
    )
    .unwrap();
    assert!(validate_xray_service_asset(&paths, InitSystem::Systemd).is_ok());
    fs::write(
        &service,
        render_systemd_xray_unit(
            std::path::Path::new("/var/lib/xray"),
            Some(GuardMode::Enforced),
        ),
    )
    .unwrap();
    assert!(validate_xray_service_asset(&paths, InitSystem::Systemd).is_ok());
    fs::write(
        &service,
        format!(
            "{}Environment=EXTRA=1\n",
            render_systemd_xray_unit(std::path::Path::new("/var/lib/xray"), None)
        ),
    )
    .unwrap();
    assert!(validate_xray_service_asset(&paths, InitSystem::Systemd).is_err());
    fs::write(
        &service,
        render_systemd_xray_unit(std::path::Path::new("/var/lib/xray"), None),
    )
    .unwrap();
    let drop_in = paths.systemd_unit_dir().join("xray.service.d");
    ensure_dir(&drop_in).unwrap();
    fs::write(drop_in.join("override.conf"), "[Service]\nExecStart=\n").unwrap();
    assert!(validate_xray_service_asset(&paths, InitSystem::Systemd).is_err());
    fs::remove_file(drop_in.join("override.conf")).unwrap();
    fs::write(
        drop_in.join("20-xp-memory.conf"),
        concat!(
            "[Service]\n",
            "# Managed by xp-ops; use a separate drop-in for overrides\n",
            "Environment=GOMEMLIMIT=16MiB\n",
            "Environment=GOGC=50\n",
        ),
    )
    .unwrap();
    assert!(validate_xray_service_asset(&paths, InitSystem::Systemd).is_ok());
}
