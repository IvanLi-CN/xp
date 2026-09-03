use crate::ops::paths::Paths;
use crate::ops::util::is_test_root;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const SERVICE_READY_TIMEOUT: Duration = Duration::from_secs(45);
const SERVICE_READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const OPENRC_READY_CONFIRMATIONS: u8 = 2;

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
    restart_service(paths, systemd_unit, openrc_service)
}

pub fn restart_xp_service(paths: &Paths) -> bool {
    restart_service(paths, "xp.service", "xp")
}

pub fn start_xp_service(paths: &Paths) -> bool {
    start_service(paths, "xp.service", "xp")
}

pub fn stop_xp_service(paths: &Paths) -> bool {
    stop_service(paths, "xp.service", "xp")
}

pub fn restart_cloudflared_service(paths: &Paths) -> bool {
    let systemd_service = paths.systemd_unit_dir().join("cloudflared.service");
    let openrc_service = paths.openrc_initd_dir().join("cloudflared");
    if !systemd_service.exists() && !openrc_service.exists() {
        return true;
    }
    restart_service(paths, "cloudflared.service", "cloudflared")
}

fn restart_service(paths: &Paths, systemd_unit: &str, openrc_service: &str) -> bool {
    if service_commands_are_disabled(paths) {
        return true;
    }
    if command_succeeds("systemctl", &["restart", systemd_unit]) {
        return wait_for_service_ready("systemctl", &["is-active", "--quiet", systemd_unit]);
    }
    if command_succeeds("rc-service", &[openrc_service, "restart"]) {
        return wait_for_service_ready("rc-service", &[openrc_service, "status"]);
    }
    false
}

fn start_service(paths: &Paths, systemd_unit: &str, openrc_service: &str) -> bool {
    if service_commands_are_disabled(paths) {
        return true;
    }
    if command_succeeds("systemctl", &["start", systemd_unit]) {
        return wait_for_service_ready("systemctl", &["is-active", "--quiet", systemd_unit]);
    }
    if command_succeeds("rc-service", &[openrc_service, "start"]) {
        return wait_for_service_ready("rc-service", &[openrc_service, "status"]);
    }
    false
}

fn stop_service(paths: &Paths, systemd_unit: &str, openrc_service: &str) -> bool {
    if service_commands_are_disabled(paths) {
        return true;
    }
    if command_succeeds("systemctl", &["stop", systemd_unit]) {
        return wait_for_service_stopped("systemctl", &["is-active", "--quiet", systemd_unit]);
    }
    if command_succeeds("rc-service", &[openrc_service, "stop"]) {
        return wait_for_service_stopped("rc-service", &[openrc_service, "status"]);
    }
    false
}

fn wait_for_service_ready(program: &str, args: &[&str]) -> bool {
    let deadline = Instant::now() + service_ready_timeout();
    let required_confirmations = service_status_confirmations(program);
    let mut ready_confirmations = 0;
    loop {
        if command_succeeds(program, args) {
            ready_confirmations += 1;
            if ready_confirmations >= required_confirmations {
                return true;
            }
        } else {
            ready_confirmations = 0;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(SERVICE_READY_POLL_INTERVAL);
    }
}

fn wait_for_service_stopped(program: &str, args: &[&str]) -> bool {
    let deadline = Instant::now() + service_ready_timeout();
    let required_confirmations = service_status_confirmations(program);
    let mut stopped_confirmations = 0;
    loop {
        if !command_succeeds(program, args) {
            stopped_confirmations += 1;
            if stopped_confirmations >= required_confirmations {
                return true;
            }
        } else {
            stopped_confirmations = 0;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(SERVICE_READY_POLL_INTERVAL);
    }
}

fn service_status_confirmations(program: &str) -> u8 {
    if program == "rc-service" {
        OPENRC_READY_CONFIRMATIONS
    } else {
        1
    }
}

fn service_ready_timeout() -> Duration {
    if cfg!(debug_assertions)
        && let Ok(timeout_ms) = std::env::var("XP_OPS_TEST_SERVICE_READY_TIMEOUT_MS")
        && let Ok(timeout_ms) = timeout_ms.parse::<u64>()
    {
        return Duration::from_millis(timeout_ms);
    }
    SERVICE_READY_TIMEOUT
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
