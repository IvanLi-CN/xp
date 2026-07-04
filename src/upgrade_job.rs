use crate::ops::cli::ExitError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const UPGRADE_DIR: &str = "upgrade";
const LOCK_FILE: &str = "start.lock";
const REQUEST_FILE: &str = "request.json";
const STATUS_FILE: &str = "status.json";
const SYSTEMD_UPGRADE_UNIT: &str = "xp-upgrade.service";
const OPENRC_UPGRADE_SERVICE: &str = "xp-upgrade";
const OPENRC_RC_SERVICE: &str = "/sbin/rc-service";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeJobState {
    Idle,
    Running,
    Restarting,
    Succeeded,
    Failed,
    Unsupported,
}

impl UpgradeJobState {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Restarting)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeRequest {
    pub target_tag: String,
    pub repo: Option<String>,
    pub requested_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeJobStatus {
    pub state: UpgradeJobState,
    pub target_tag: Option<String>,
    pub repo: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub message: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeSupport {
    pub supported: bool,
    pub reason: Option<String>,
    pub trigger: Option<&'static str>,
}

#[derive(Debug)]
pub enum UpgradeStartError {
    Active,
    Unsupported(String),
    InvalidTarget(String),
    Io(io::Error),
    TriggerFailed(String),
}

impl From<io::Error> for UpgradeStartError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn request_path(data_dir: &Path) -> PathBuf {
    data_dir.join(UPGRADE_DIR).join(REQUEST_FILE)
}

pub fn status_path(data_dir: &Path) -> PathBuf {
    data_dir.join(UPGRADE_DIR).join(STATUS_FILE)
}

pub fn upgrade_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(UPGRADE_DIR)
}

fn lock_path(data_dir: &Path) -> PathBuf {
    upgrade_dir(data_dir).join(LOCK_FILE)
}

struct StartLock {
    path: PathBuf,
}

impl StartLock {
    fn acquire(data_dir: &Path) -> Result<Self, UpgradeStartError> {
        let dir = upgrade_dir(data_dir);
        fs::create_dir_all(&dir)?;
        let path = lock_path(data_dir);
        match File::options().write(true).create_new(true).open(&path) {
            Ok(_) => Ok(Self { path }),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                Err(UpgradeStartError::Active)
            }
            Err(err) => Err(UpgradeStartError::Io(err)),
        }
    }
}

impl Drop for StartLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn idle_status() -> UpgradeJobStatus {
    let now = now_rfc3339();
    UpgradeJobStatus {
        state: UpgradeJobState::Idle,
        target_tag: None,
        repo: None,
        started_at: None,
        finished_at: None,
        exit_code: None,
        message: None,
        updated_at: now,
    }
}

pub fn read_status(data_dir: &Path) -> io::Result<UpgradeJobStatus> {
    let path = status_path(data_dir);
    if !path.exists() {
        return Ok(idle_status());
    }
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(io::Error::other)
}

pub fn write_status(data_dir: &Path, status: &UpgradeJobStatus) -> io::Result<()> {
    let dir = upgrade_dir(data_dir);
    fs::create_dir_all(&dir)?;
    let path = status_path(data_dir);
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(status).map_err(io::Error::other)?;
    fs::write(&tmp, raw + "\n")?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn read_request(data_dir: &Path) -> Result<UpgradeRequest, ExitError> {
    let raw = fs::read_to_string(request_path(data_dir))
        .map_err(|e| ExitError::new(3, format!("invalid_args: read upgrade request: {e}")))?;
    serde_json::from_str(&raw)
        .map_err(|e| ExitError::new(3, format!("invalid_args: parse upgrade request: {e}")))
}

fn write_request(data_dir: &Path, request: &UpgradeRequest) -> io::Result<()> {
    let dir = upgrade_dir(data_dir);
    fs::create_dir_all(&dir)?;
    let path = request_path(data_dir);
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(request).map_err(io::Error::other)?;
    fs::write(&tmp, raw + "\n")?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn support_status() -> UpgradeSupport {
    if let Some(trigger) = test_forced_host_trigger() {
        return UpgradeSupport {
            supported: true,
            reason: None,
            trigger: Some(trigger),
        };
    }

    if is_container_runtime() {
        return UpgradeSupport {
            supported: false,
            reason: Some(
                "web upgrades are only supported on host-managed systemd/OpenRC nodes".to_string(),
            ),
            trigger: None,
        };
    }

    if command_exists("systemctl") {
        return UpgradeSupport {
            supported: true,
            reason: None,
            trigger: Some("systemd"),
        };
    }

    if command_exists("doas") && command_exists(OPENRC_RC_SERVICE) {
        return UpgradeSupport {
            supported: true,
            reason: None,
            trigger: Some("openrc"),
        };
    }

    UpgradeSupport {
        supported: false,
        reason: Some("missing supported upgrade trigger: systemctl or doas+rc-service".to_string()),
        trigger: None,
    }
}

fn test_forced_host_trigger() -> Option<&'static str> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("XP_UPGRADE_TEST_FORCE_HOST_TRIGGER")
        .ok()?
        .as_str()
    {
        "systemd" => Some("systemd"),
        "openrc" => Some("openrc"),
        _ => None,
    }
}

pub fn start_upgrade(
    data_dir: &Path,
    target_tag: &str,
    repo: Option<String>,
) -> Result<UpgradeJobStatus, UpgradeStartError> {
    validate_target_tag(target_tag)?;
    let _lock = StartLock::acquire(data_dir)?;

    let current = read_status(data_dir)?;
    if current.state.is_active() {
        return Err(UpgradeStartError::Active);
    }

    let support = support_status();
    if !support.supported {
        let reason = support
            .reason
            .unwrap_or_else(|| "upgrade trigger is not supported".to_string());
        let now = now_rfc3339();
        let status = UpgradeJobStatus {
            state: UpgradeJobState::Unsupported,
            target_tag: Some(target_tag.to_string()),
            repo,
            started_at: None,
            finished_at: Some(now.clone()),
            exit_code: None,
            message: Some(reason.clone()),
            updated_at: now,
        };
        write_status(data_dir, &status)?;
        return Err(UpgradeStartError::Unsupported(reason));
    }

    let now = now_rfc3339();
    let request = UpgradeRequest {
        target_tag: target_tag.to_string(),
        repo: repo.clone(),
        requested_at: now.clone(),
    };
    write_request(data_dir, &request)?;

    let status = UpgradeJobStatus {
        state: UpgradeJobState::Running,
        target_tag: Some(target_tag.to_string()),
        repo,
        started_at: Some(now.clone()),
        finished_at: None,
        exit_code: None,
        message: Some("upgrade trigger accepted".to_string()),
        updated_at: now,
    };
    write_status(data_dir, &status)?;

    if let Err(message) = trigger_upgrade_service(support.trigger) {
        let now = now_rfc3339();
        let failed = UpgradeJobStatus {
            state: UpgradeJobState::Failed,
            target_tag: status.target_tag.clone(),
            repo: status.repo.clone(),
            started_at: status.started_at.clone(),
            finished_at: Some(now.clone()),
            exit_code: None,
            message: Some(message.clone()),
            updated_at: now,
        };
        let _ = write_status(data_dir, &failed);
        return Err(UpgradeStartError::TriggerFailed(message));
    }

    Ok(status)
}

pub fn status_for_runner_start(request: &UpgradeRequest) -> UpgradeJobStatus {
    let now = now_rfc3339();
    UpgradeJobStatus {
        state: UpgradeJobState::Restarting,
        target_tag: Some(request.target_tag.clone()),
        repo: request.repo.clone(),
        started_at: Some(request.requested_at.clone()),
        finished_at: None,
        exit_code: None,
        message: Some("xp-ops upgrade is running; xp may restart".to_string()),
        updated_at: now,
    }
}

pub fn status_for_runner_finish(
    request: &UpgradeRequest,
    result: Result<(), &ExitError>,
) -> UpgradeJobStatus {
    let now = now_rfc3339();
    match result {
        Ok(()) => UpgradeJobStatus {
            state: UpgradeJobState::Succeeded,
            target_tag: Some(request.target_tag.clone()),
            repo: request.repo.clone(),
            started_at: Some(request.requested_at.clone()),
            finished_at: Some(now.clone()),
            exit_code: Some(0),
            message: Some("upgrade completed".to_string()),
            updated_at: now,
        },
        Err(err) => UpgradeJobStatus {
            state: UpgradeJobState::Failed,
            target_tag: Some(request.target_tag.clone()),
            repo: request.repo.clone(),
            started_at: Some(request.requested_at.clone()),
            finished_at: Some(now.clone()),
            exit_code: Some(err.code),
            message: Some(err.message.clone()),
            updated_at: now,
        },
    }
}

fn validate_target_tag(target_tag: &str) -> Result<(), UpgradeStartError> {
    let value = target_tag.trim();
    if value.is_empty() || value.len() > 80 {
        return Err(UpgradeStartError::InvalidTarget(
            "target_tag must be a non-empty release tag".to_string(),
        ));
    }
    let valid = value.starts_with('v')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, 'v' | '.' | '-' | '_'));
    if !valid {
        return Err(UpgradeStartError::InvalidTarget(
            "target_tag must be a v-prefixed release tag".to_string(),
        ));
    }
    Ok(())
}

fn trigger_upgrade_service(trigger: Option<&str>) -> Result<(), String> {
    match trigger {
        Some("systemd") => run_status(Command::new("systemctl").args([
            "start",
            "--no-block",
            SYSTEMD_UPGRADE_UNIT,
        ])),
        Some("openrc") => run_status(Command::new("doas").args(openrc_trigger_args())),
        _ => Err("upgrade trigger is not supported".to_string()),
    }
}

fn openrc_trigger_args() -> [&'static str; 3] {
    [OPENRC_RC_SERVICE, OPENRC_UPGRADE_SERVICE, "start"]
}

fn run_status(cmd: &mut Command) -> Result<(), String> {
    let program = cmd.get_program().to_string_lossy().to_string();
    let status = cmd
        .status()
        .map_err(|e| format!("trigger_failed: {program}: {e}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!(
        "trigger_failed: {program} exit={}",
        status.code().unwrap_or(-1)
    ))
}

fn command_exists(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program} >/dev/null 2>&1")])
        .status()
        .ok()
        .is_some_and(|status| status.success())
}

fn is_container_runtime() -> bool {
    if std::env::var_os("XP_CONTAINER_NAME").is_some()
        || std::env::var_os("XP_OPS_CONTAINER_XP_BIN").is_some()
    {
        return true;
    }
    if Path::new("/.dockerenv").exists() {
        return true;
    }
    let Ok(cgroup) = fs::read_to_string("/proc/1/cgroup") else {
        return false;
    };
    cgroup.contains("docker") || cgroup.contains("containerd") || cgroup.contains("kubepods")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn status_round_trips() {
        let tmp = tempdir().unwrap();
        let status = UpgradeJobStatus {
            state: UpgradeJobState::Running,
            target_tag: Some("v0.2.0".to_string()),
            repo: Some("IvanLi-CN/xp".to_string()),
            started_at: Some("2026-07-04T00:00:00Z".to_string()),
            finished_at: None,
            exit_code: None,
            message: Some("running".to_string()),
            updated_at: "2026-07-04T00:00:00Z".to_string(),
        };

        write_status(tmp.path(), &status).unwrap();
        let loaded = read_status(tmp.path()).unwrap();

        assert_eq!(loaded.state, UpgradeJobState::Running);
        assert_eq!(loaded.target_tag.as_deref(), Some("v0.2.0"));
    }

    #[test]
    fn missing_status_is_idle() {
        let tmp = tempdir().unwrap();
        let loaded = read_status(tmp.path()).unwrap();
        assert_eq!(loaded.state, UpgradeJobState::Idle);
    }

    #[test]
    fn invalid_target_is_rejected() {
        let err = validate_target_tag("latest").unwrap_err();
        assert!(matches!(err, UpgradeStartError::InvalidTarget(_)));
    }

    #[test]
    fn start_lock_rejects_concurrent_claim() {
        let tmp = tempdir().unwrap();
        let _lock = StartLock::acquire(tmp.path()).unwrap();
        let err = start_upgrade(tmp.path(), "v0.2.0", None).unwrap_err();
        assert!(matches!(err, UpgradeStartError::Active));
    }

    #[test]
    fn openrc_trigger_matches_installed_doas_policy() {
        assert_eq!(
            openrc_trigger_args(),
            ["/sbin/rc-service", "xp-upgrade", "start"]
        );
    }
}
