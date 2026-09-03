use crate::ops::paths::Paths;
use crate::ops::util::is_test_root;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const SERVICE_READY_TIMEOUT: Duration = Duration::from_secs(45);
const SERVICE_READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const OPENRC_STATUS_CONFIRMATIONS: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpenrcServiceState {
    Started,
    Stopped,
    Starting,
    Stopping,
    Unknown,
}

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
    restart_openrc_service(openrc_service)
}

fn start_service(paths: &Paths, systemd_unit: &str, openrc_service: &str) -> bool {
    if service_commands_are_disabled(paths) {
        return true;
    }
    if command_succeeds("systemctl", &["start", systemd_unit]) {
        return wait_for_service_ready("systemctl", &["is-active", "--quiet", systemd_unit]);
    }
    start_openrc_service(openrc_service)
}

fn stop_service(paths: &Paths, systemd_unit: &str, openrc_service: &str) -> bool {
    if service_commands_are_disabled(paths) {
        return true;
    }
    if command_succeeds("systemctl", &["stop", systemd_unit]) {
        return wait_for_service_stopped("systemctl", &["is-active", "--quiet", systemd_unit]);
    }
    stop_openrc_service(openrc_service)
}

fn restart_openrc_service(service: &str) -> bool {
    stop_openrc_service(service) && start_openrc_service(service)
}

fn start_openrc_service(service: &str) -> bool {
    command_runs("rc-service", &[service, "start"])
        && wait_for_openrc_service_state(service, OpenrcServiceState::Started)
}

fn stop_openrc_service(service: &str) -> bool {
    command_runs("rc-service", &[service, "stop"])
        && wait_for_openrc_service_state(service, OpenrcServiceState::Stopped)
}

fn wait_for_service_ready(program: &str, args: &[&str]) -> bool {
    let deadline = Instant::now() + service_ready_timeout();
    loop {
        if command_succeeds(program, args) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(SERVICE_READY_POLL_INTERVAL);
    }
}

fn wait_for_service_stopped(program: &str, args: &[&str]) -> bool {
    let deadline = Instant::now() + service_ready_timeout();
    loop {
        if !command_succeeds(program, args) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(SERVICE_READY_POLL_INTERVAL);
    }
}

fn wait_for_openrc_service_state(service: &str, expected: OpenrcServiceState) -> bool {
    let deadline = Instant::now() + service_ready_timeout();
    let mut confirmations = 0;
    loop {
        if openrc_service_state(service) == expected {
            confirmations += 1;
            if confirmations >= OPENRC_STATUS_CONFIRMATIONS {
                return true;
            }
        } else {
            confirmations = 0;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(SERVICE_READY_POLL_INTERVAL);
    }
}

fn openrc_service_state(service: &str) -> OpenrcServiceState {
    let Ok(output) = Command::new("rc-service")
        .args([service, "status"])
        .output()
    else {
        return OpenrcServiceState::Unknown;
    };

    parse_openrc_service_state(&String::from_utf8_lossy(&output.stdout))
        .or_else(|| parse_openrc_service_state(&String::from_utf8_lossy(&output.stderr)))
        .unwrap_or(OpenrcServiceState::Unknown)
}

// OpenRC uses nonzero exit statuses for both terminal and transitional states.
fn parse_openrc_service_state(output: &str) -> Option<OpenrcServiceState> {
    output.lines().find_map(|line| {
        let normalized = line.trim().to_ascii_lowercase();
        let (_, state) = normalized.split_once("status:")?;
        match state.split_whitespace().next()? {
            "started" => Some(OpenrcServiceState::Started),
            "stopped" => Some(OpenrcServiceState::Stopped),
            "starting" => Some(OpenrcServiceState::Starting),
            "stopping" => Some(OpenrcServiceState::Stopping),
            _ => None,
        }
    })
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

fn command_runs(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).status().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openrc_stopping_as_a_distinct_nonterminal_state() {
        assert_eq!(
            parse_openrc_service_state(" * status: stopping\n"),
            Some(OpenrcServiceState::Stopping)
        );
        assert_eq!(
            parse_openrc_service_state(" * status: stopped\n"),
            Some(OpenrcServiceState::Stopped)
        );
    }

    #[test]
    fn parses_openrc_started_and_unknown_statuses() {
        assert_eq!(
            parse_openrc_service_state(" * status: started\n"),
            Some(OpenrcServiceState::Started)
        );
        assert_eq!(parse_openrc_service_state("status: crashed\n"), None);
    }
}
