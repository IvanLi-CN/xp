use super::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn openrc_upgrade_policy_only_allows_readiness_check_and_runner_start() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.etc_doas_conf().parent().unwrap()).unwrap();
    fs::write(paths.etc_doas_conf(), "permit nopass root\n").unwrap();

    write_openrc_upgrade_policy(&paths, Mode::Real).unwrap();

    let doas = fs::read_to_string(paths.etc_doas_conf()).unwrap();
    assert!(doas.contains("permit nopass root"));
    assert!(doas.contains(
        "permit nopass xp as root cmd /usr/local/libexec/xp-openrc-upgrade-trigger args --check"
    ));
    assert!(doas.contains(
        "permit nopass xp as root cmd /usr/local/libexec/xp-openrc-upgrade-trigger args start"
    ));
    assert!(!doas.contains("cmd /sbin/rc-service args xp-upgrade start"));
    assert!(!doas.contains("xp-upgrade restart"));

    let script = openrc_xp_upgrade_script();
    assert!(script.contains("xp-ops _upgrade-runner"));
    assert!(script.contains(r#""${XP_DATA_DIR:-/var/lib/xp/data}""#));
    assert!(script.contains("command_background=\"yes\""));
    assert!(script.contains("pidfile=\"/run/xp-upgrade.pid\""));
    assert!(script.contains("rc-service xp-upgrade zap"));
}

#[test]
fn openrc_upgrade_policy_backfills_check_without_duplicating_start_rule() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    fs::create_dir_all(paths.etc_doas_conf().parent().unwrap()).unwrap();
    fs::write(
        paths.etc_doas_conf(),
        concat!(
            "# Managed by xp-ops: allow xp to start the upgrade runner\n",
            "permit nopass operator as root cmd /usr/bin/id\n",
            "permit nopass xp as root cmd /sbin/rc-service args xp-upgrade start\n",
        ),
    )
    .unwrap();

    write_openrc_upgrade_policy(&paths, Mode::Real).unwrap();
    write_openrc_upgrade_policy(&paths, Mode::Real).unwrap();

    let doas = fs::read_to_string(paths.etc_doas_conf()).unwrap();
    assert!(doas.contains("permit nopass operator as root cmd /usr/bin/id"));
    assert!(doas.contains("# Managed by xp-ops: allow xp to start the upgrade runner"));
    assert!(!doas.contains("# Managed by xp-ops: allow xp to check and start the upgrade runner"));
    assert!(!doas.contains("cmd /sbin/rc-service args xp-upgrade start"));
    assert_eq!(
        doas.matches(
            "permit nopass xp as root cmd /usr/local/libexec/xp-openrc-upgrade-trigger args --check"
        )
        .count(),
        1
    );
    assert_eq!(
        doas.matches(
            "permit nopass xp as root cmd /usr/local/libexec/xp-openrc-upgrade-trigger args start"
        )
        .count(),
        1
    );
}

#[test]
fn openrc_upgrade_trigger_helper_only_checks_fixed_assets() {
    let tmp = tempdir().unwrap();
    let paths = Paths::new(tmp.path().to_path_buf());
    write_openrc_upgrade_trigger_delegate(&paths, Mode::Real).unwrap();

    let helper_path = paths.usr_local_libexec_xp_openrc_upgrade_trigger();
    let helper = fs::read_to_string(&helper_path).unwrap();
    assert_eq!(
        helper,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/ops/openrc/xp-upgrade-trigger"
        ))
    );
    assert!(helper.contains("[ \"$#\" -ne 1 ]"));
    assert!(helper.contains("[ -x /etc/init.d/xp-upgrade ]"));
    assert!(helper.contains("grep -Fqx"));
    assert!(helper.contains("rc-service xp-upgrade start"));
    assert!(helper.contains("rc-service xp-upgrade zap"));
    assert!(!helper.contains("$@"));
}
