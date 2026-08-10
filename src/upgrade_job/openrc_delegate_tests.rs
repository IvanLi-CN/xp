use super::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[cfg(unix)]
static OPENRC_DOAS_PROGRAM_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(unix)]
fn with_openrc_doas_program<T>(program: &Path, action: impl FnOnce() -> T) -> T {
    struct RestoreProgram(Option<std::ffi::OsString>);

    impl Drop for RestoreProgram {
        fn drop(&mut self) {
            match self.0.take() {
                Some(program) => unsafe {
                    std::env::set_var("XP_UPGRADE_TEST_OPENRC_DOAS_PROGRAM", program)
                },
                None => unsafe { std::env::remove_var("XP_UPGRADE_TEST_OPENRC_DOAS_PROGRAM") },
            }
        }
    }

    let _guard = OPENRC_DOAS_PROGRAM_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let original_program = std::env::var_os("XP_UPGRADE_TEST_OPENRC_DOAS_PROGRAM");
    unsafe { std::env::set_var("XP_UPGRADE_TEST_OPENRC_DOAS_PROGRAM", program) };
    let _restore = RestoreProgram(original_program);
    action()
}

#[test]
fn openrc_delegate_assets_require_runner_and_helper() {
    let tmp = tempdir().unwrap();
    let openrc_script = root_abs(tmp.path(), OPENRC_UPGRADE_SCRIPT_PATH);
    fs::create_dir_all(openrc_script.parent().unwrap()).unwrap();
    fs::write(openrc_script, "script").unwrap();
    assert!(!openrc_upgrade_delegate_assets_installed(tmp.path()));

    let openrc_helper = root_abs(tmp.path(), OPENRC_UPGRADE_TRIGGER_PATH);
    fs::create_dir_all(openrc_helper.parent().unwrap()).unwrap();
    fs::write(openrc_helper, "helper").unwrap();
    assert!(openrc_upgrade_delegate_assets_installed(tmp.path()));
}

#[cfg(unix)]
#[test]
fn openrc_delegate_probe_accepts_root_only_doas_policy() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let openrc_script = root_abs(tmp.path(), OPENRC_UPGRADE_SCRIPT_PATH);
    let openrc_helper = root_abs(tmp.path(), OPENRC_UPGRADE_TRIGGER_PATH);
    let doas_conf = root_abs(tmp.path(), "/etc/doas.conf");
    for path in [&openrc_script, &openrc_helper, &doas_conf] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
    }
    fs::write(openrc_script, "script").unwrap();
    fs::write(openrc_helper, "helper").unwrap();
    fs::write(
        &doas_conf,
        concat!(
            "permit nopass xp as root cmd /usr/local/libexec/",
            "xp-openrc-upgrade-trigger args --check\n",
            "permit nopass xp as root cmd /usr/local/libexec/",
            "xp-openrc-upgrade-trigger args start\n",
        ),
    )
    .unwrap();
    fs::set_permissions(&doas_conf, fs::Permissions::from_mode(0o000)).unwrap();

    let bin = tempdir().unwrap();
    let doas = bin.path().join("doas");
    fs::write(
        &doas,
        concat!(
            "#!/bin/sh\n",
            "[ \"$#\" -eq 3 ] && \\\n",
            "[ \"$1\" = \"-n\" ] && \\\n",
            "[ \"$2\" = \"/usr/local/libexec/xp-openrc-upgrade-trigger\" ] && \\\n",
            "[ \"$3\" = \"--check\" ]\n",
        ),
    )
    .unwrap();
    fs::set_permissions(&doas, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(with_openrc_doas_program(&doas, || {
        openrc_upgrade_delegate_installed(tmp.path())
    }));
}

#[cfg(unix)]
#[test]
fn openrc_delegate_probe_rejects_denied_check() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    for path in [
        root_abs(tmp.path(), OPENRC_UPGRADE_SCRIPT_PATH),
        root_abs(tmp.path(), OPENRC_UPGRADE_TRIGGER_PATH),
    ] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "helper").unwrap();
    }

    let bin = tempdir().unwrap();
    let doas = bin.path().join("doas");
    fs::write(&doas, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&doas, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(!with_openrc_doas_program(&doas, || {
        openrc_upgrade_delegate_installed(tmp.path())
    }));
}

#[test]
fn openrc_trigger_matches_installed_doas_policy() {
    assert_eq!(
        openrc_doas_check_args(),
        [
            "-n",
            "/usr/local/libexec/xp-openrc-upgrade-trigger",
            "--check"
        ]
    );
    assert_eq!(
        openrc_trigger_args(),
        [
            "-n",
            "/usr/local/libexec/xp-openrc-upgrade-trigger",
            "start"
        ]
    );
}
