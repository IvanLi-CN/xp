use crate::ops::paths::Paths;
use crate::ops::util::is_test_root;
use std::process::Command;

pub fn reload_systemd_units(paths: &Paths) -> bool {
    let systemd = paths.systemd_unit_dir();
    if !systemd.join("xray.service").exists() && !systemd.join("cloudflared.service").exists() {
        return true;
    }
    if service_commands_are_disabled(paths) {
        return true;
    }
    command_succeeds("systemctl", &["daemon-reload"])
}

pub fn restart_xray_service(paths: &Paths, systemd_unit: &str, openrc_service: &str) -> bool {
    if service_commands_are_disabled(paths) {
        return true;
    }
    command_succeeds("systemctl", &["restart", systemd_unit])
        || command_succeeds("rc-service", &[openrc_service, "restart"])
}

pub fn restart_cloudflared_service(paths: &Paths) -> bool {
    let systemd_service = paths.systemd_unit_dir().join("cloudflared.service");
    let openrc_service = paths.openrc_initd_dir().join("cloudflared");
    if !systemd_service.exists() && !openrc_service.exists() {
        return true;
    }
    if service_commands_are_disabled(paths) {
        return true;
    }
    (systemd_service.exists() && command_succeeds("systemctl", &["restart", "cloudflared.service"]))
        || (openrc_service.exists() && command_succeeds("rc-service", &["cloudflared", "restart"]))
}

fn service_commands_are_disabled(paths: &Paths) -> bool {
    is_test_root(paths.root()) && !test_enable_service_restart()
}

fn test_enable_service_restart() -> bool {
    cfg!(debug_assertions)
        && matches!(
            std::env::var("XP_OPS_TEST_ENABLE_SERVICE").as_deref(),
            Ok("1") | Ok("true") | Ok("TRUE")
        )
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .status()
        .ok()
        .is_some_and(|status| status.success())
}
