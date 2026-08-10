use super::*;
use std::collections::HashMap;

pub(super) fn detect_upgrade_delegate_failure(status: &UpgradeJobStatus) -> Option<String> {
    detect_systemd_upgrade_failure().or_else(|| detect_openrc_upgrade_failure(status))
}

fn detect_openrc_upgrade_failure(status: &UpgradeJobStatus) -> Option<String> {
    if !command_exists("rc-service") {
        return None;
    }
    let output = Command::new(OPENRC_RC_SERVICE)
        .args([OPENRC_UPGRADE_SERVICE, "status"])
        .output()
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    openrc_failure_message(&text, status, chrono::Utc::now())
}

pub(super) fn openrc_failure_message(
    status_output: &str,
    status: &UpgradeJobStatus,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    let output = status_output.to_ascii_lowercase();
    if output.contains("crashed") {
        return Some(format!(
            "upgrade runner failed: {OPENRC_UPGRADE_SERVICE} is crashed"
        ));
    }
    if !output.contains("stopped") {
        return None;
    }
    let updated_at = chrono::DateTime::parse_from_rfc3339(&status.updated_at).ok()?;
    (now.signed_duration_since(updated_at.with_timezone(&chrono::Utc))
        .num_seconds()
        >= 10)
        .then(|| {
            format!("upgrade runner failed: {OPENRC_UPGRADE_SERVICE} stopped before completion")
        })
}

fn detect_systemd_upgrade_failure() -> Option<String> {
    if !command_exists("systemctl") {
        return None;
    }
    let output = Command::new("systemctl")
        .args([
            "show",
            SYSTEMD_UPGRADE_UNIT,
            "--property=LoadState,ActiveState,SubState,Result,ExecMainStatus",
            "--no-pager",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let fields = parse_systemctl_show(&String::from_utf8_lossy(&output.stdout));
    if fields.get("LoadState").map(String::as_str) != Some("loaded")
        || fields.get("ActiveState").map(String::as_str) != Some("failed")
    {
        return None;
    }
    let result = fields
        .get("Result")
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let status = fields
        .get("ExecMainStatus")
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    Some(format!(
        "upgrade runner failed: {SYSTEMD_UPGRADE_UNIT} is failed \
         (result={result}, exit_status={status})"
    ))
}

pub(super) fn parse_systemctl_show(raw: &str) -> HashMap<String, String> {
    raw.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}
