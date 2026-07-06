use crate::ops::cli::ExitError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const UPGRADE_DIR: &str = "upgrade";
const LOCK_FILE: &str = "start.lock";
const REQUEST_FILE: &str = "request.json";
const STATUS_FILE: &str = "status.json";
const SYSTEMD_UPGRADE_UNIT: &str = "xp-upgrade.service";
const SYSTEMD_UPGRADE_UNIT_PATH: &str = "/etc/systemd/system/xp-upgrade.service";
const SYSTEMD_UPGRADE_TRIGGER_PATH: &str = "/usr/local/libexec/xp-upgrade-trigger";
const SYSTEMD_SUDOERS_PATH: &str = "/etc/sudoers.d/91-xp-upgrade";
const SYSTEMD_POLKIT_RULE_PATH: &str = "/etc/polkit-1/rules.d/91-xp-upgrade.rules";
const SYSTEMD_POLKIT_ACTION: &str = "org.freedesktop.systemd1.manage-units";
const SYSTEMD_UPGRADE_POLKIT_UNIT_RULE: &str = r#"unit == "xp-upgrade.service""#;
const SYSTEMD_UPGRADE_POLKIT_VERB_RULE: &str = r#"verb == "start""#;
const SYSTEMD_UPGRADE_SUDOERS_START_RULE: &str =
    "xp ALL=(root) NOPASSWD: /usr/local/libexec/xp-upgrade-trigger \"\"";
const SYSTEMD_UPGRADE_SUDOERS_CHECK_RULE: &str =
    "xp ALL=(root) NOPASSWD: /usr/local/libexec/xp-upgrade-trigger --check";
const OPENRC_UPGRADE_SERVICE: &str = "xp-upgrade";
const OPENRC_RC_SERVICE: &str = "/sbin/rc-service";
const OPENRC_UPGRADE_SCRIPT_PATH: &str = "/etc/init.d/xp-upgrade";
const OPENRC_DOAS_CONF_PATH: &str = "/etc/doas.conf";
const OPENRC_UPGRADE_DOAS_RULE: &str =
    "permit nopass xp as root cmd /sbin/rc-service args xp-upgrade start";

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

pub fn read_reconciled_status(data_dir: &Path) -> io::Result<UpgradeJobStatus> {
    let status = read_status(data_dir)?;
    reconcile_active_status(data_dir, status, detect_upgrade_delegate_failure)
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

pub fn root_controlled_runner_repo(
    request_repo: Option<&str>,
    default_repo: &str,
) -> Result<Option<String>, ExitError> {
    let runner_repo = std::env::var("XP_OPS_GITHUB_REPO")
        .ok()
        .map(|v| v.trim().trim_matches('/').to_string())
        .filter(|v| !v.is_empty());
    if request_repo != runner_repo.as_deref()
        && !(request_repo == Some(default_repo) && runner_repo.is_none())
    {
        return Err(ExitError::new(
            3,
            "invalid_args: upgrade repo must match root runner XP_OPS_GITHUB_REPO",
        ));
    }
    Ok(runner_repo)
}

pub fn prepare_runner_request(
    data_dir: &Path,
    default_repo: &str,
) -> Result<UpgradeRequest, ExitError> {
    let mut request = read_request(data_dir)?;
    match validate_runner_request(&request, default_repo) {
        Ok(repo) => {
            request.repo = repo;
            Ok(request)
        }
        Err(err) => {
            let failed = status_for_runner_finish(&request, Err(&err));
            let _ = write_status(data_dir, &failed);
            Err(err)
        }
    }
}

fn validate_runner_request(
    request: &UpgradeRequest,
    default_repo: &str,
) -> Result<Option<String>, ExitError> {
    validate_target_tag_for_runner(&request.target_tag)?;
    root_controlled_runner_repo(request.repo.as_deref(), default_repo)
}

fn validate_target_tag_for_runner(target_tag: &str) -> Result<(), ExitError> {
    validate_target_tag(target_tag).map_err(|err| match err {
        UpgradeStartError::InvalidTarget(message) => {
            ExitError::new(3, format!("invalid_args: {message}"))
        }
        UpgradeStartError::Active => ExitError::new(3, "invalid_args: active upgrade job"),
        UpgradeStartError::Unsupported(message) => {
            ExitError::new(3, format!("invalid_args: {message}"))
        }
        UpgradeStartError::Io(err) => {
            ExitError::new(7, format!("service_error: validate upgrade target: {err}"))
        }
        UpgradeStartError::TriggerFailed(message) => {
            ExitError::new(7, format!("service_error: {message}"))
        }
    })
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
    support_status_for_root(Path::new("/"))
}

fn support_status_for_root(root: &Path) -> UpgradeSupport {
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

    if command_exists("systemctl") && systemd_upgrade_delegate_installed(root) {
        return UpgradeSupport {
            supported: true,
            reason: None,
            trigger: Some("systemd"),
        };
    }

    if command_exists("doas")
        && command_exists(OPENRC_RC_SERVICE)
        && openrc_upgrade_delegate_installed(root)
    {
        return UpgradeSupport {
            supported: true,
            reason: None,
            trigger: Some("openrc"),
        };
    }

    UpgradeSupport {
        supported: false,
        reason: Some(
            "missing installed upgrade delegate; rerun xp-ops init on this host".to_string(),
        ),
        trigger: None,
    }
}

fn systemd_upgrade_delegate_installed(root: &Path) -> bool {
    systemd_upgrade_delegate_installed_with_polkit_file(root, root != Path::new("/"))
}

fn systemd_upgrade_delegate_installed_with_polkit_file(
    root: &Path,
    allow_static_polkit_file: bool,
) -> bool {
    if !root_abs(root, SYSTEMD_UPGRADE_UNIT_PATH).exists() {
        return false;
    }

    if allow_static_polkit_file {
        return systemd_upgrade_sudo_delegate_installed(root)
            || systemd_upgrade_polkit_rule_readable(root);
    }

    systemd_upgrade_sudo_delegate_installed(root) || systemd_upgrade_polkit_allows_current_process()
}

fn systemd_upgrade_sudo_delegate_installed(root: &Path) -> bool {
    if !root_abs(root, SYSTEMD_UPGRADE_TRIGGER_PATH).exists() {
        return false;
    }

    if root != Path::new("/") {
        return fs::read_to_string(root_abs(root, SYSTEMD_SUDOERS_PATH))
            .ok()
            .is_some_and(|content| {
                content_has_line(&content, SYSTEMD_UPGRADE_SUDOERS_START_RULE)
                    && content_has_line(&content, SYSTEMD_UPGRADE_SUDOERS_CHECK_RULE)
            });
    }

    systemd_upgrade_sudo_helper_allows_current_process()
}

fn content_has_line(content: &str, needle: &str) -> bool {
    content.lines().any(|line| line.trim() == needle)
}

fn systemd_upgrade_sudo_helper_allows_current_process() -> bool {
    let trigger_path = systemd_upgrade_trigger_path();
    if !command_exists("sudo") || !Path::new(&trigger_path).exists() {
        return false;
    }
    sudo_status(systemd_sudo_check_args(&trigger_path))
        && sudo_status(systemd_sudo_list_start_args(&trigger_path))
}

fn sudo_status(args: Vec<String>) -> bool {
    Command::new("sudo")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn systemd_upgrade_trigger_path() -> String {
    if cfg!(debug_assertions) {
        std::env::var("XP_UPGRADE_TEST_SYSTEMD_TRIGGER_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| SYSTEMD_UPGRADE_TRIGGER_PATH.to_string())
    } else {
        SYSTEMD_UPGRADE_TRIGGER_PATH.to_string()
    }
}

fn systemd_upgrade_polkit_rule_readable(root: &Path) -> bool {
    fs::read_to_string(root_abs(root, SYSTEMD_POLKIT_RULE_PATH))
        .ok()
        .is_some_and(|content| {
            content.contains(SYSTEMD_POLKIT_ACTION)
                && content.contains(SYSTEMD_UPGRADE_POLKIT_UNIT_RULE)
                && content.contains(SYSTEMD_UPGRADE_POLKIT_VERB_RULE)
        })
}

fn systemd_upgrade_polkit_allows_current_process() -> bool {
    let pid = std::process::id().to_string();
    Command::new("pkcheck")
        .args([
            "--action-id",
            SYSTEMD_POLKIT_ACTION,
            "--process",
            &pid,
            "--detail",
            "unit",
            SYSTEMD_UPGRADE_UNIT,
            "--detail",
            "verb",
            "start",
        ])
        .status()
        .is_ok_and(|status| status.success())
}

fn openrc_upgrade_delegate_installed(root: &Path) -> bool {
    if !root_abs(root, OPENRC_UPGRADE_SCRIPT_PATH).exists() {
        return false;
    }
    fs::read_to_string(root_abs(root, OPENRC_DOAS_CONF_PATH))
        .ok()
        .is_some_and(|content| content.contains(OPENRC_UPGRADE_DOAS_RULE))
}

fn root_abs(root: &Path, absolute_path: &str) -> PathBuf {
    let path = Path::new(absolute_path);
    if root == Path::new("/") {
        return path.to_path_buf();
    }
    root.join(path.strip_prefix("/").unwrap_or(path))
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

    let current = read_reconciled_status(data_dir)?;
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
        Some("systemd") => trigger_systemd_upgrade_service(),
        Some("openrc") => run_status(Command::new("doas").args(openrc_trigger_args())),
        _ => Err("upgrade trigger is not supported".to_string()),
    }
}

fn reconcile_active_status<F>(
    data_dir: &Path,
    status: UpgradeJobStatus,
    detect_failure: F,
) -> io::Result<UpgradeJobStatus>
where
    F: FnOnce() -> Option<String>,
{
    if !status.state.is_active() {
        return Ok(status);
    }

    let Some(message) = detect_failure() else {
        return Ok(status);
    };

    let now = now_rfc3339();
    let failed = UpgradeJobStatus {
        state: UpgradeJobState::Failed,
        target_tag: status.target_tag.clone(),
        repo: status.repo.clone(),
        started_at: status.started_at.clone(),
        finished_at: Some(now.clone()),
        exit_code: status.exit_code,
        message: Some(message),
        updated_at: now,
    };
    write_status(data_dir, &failed)?;
    Ok(failed)
}

fn detect_upgrade_delegate_failure() -> Option<String> {
    detect_systemd_upgrade_failure()
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

    let text = String::from_utf8_lossy(&output.stdout);
    let fields = parse_systemctl_show(&text);
    if fields.get("LoadState").map(String::as_str) != Some("loaded") {
        return None;
    }
    if fields.get("ActiveState").map(String::as_str) != Some("failed") {
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

fn parse_systemctl_show(raw: &str) -> HashMap<String, String> {
    raw.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

fn trigger_systemd_upgrade_service() -> Result<(), String> {
    if systemd_upgrade_sudo_helper_allows_current_process() {
        return run_status(Command::new("sudo").args(systemd_sudo_trigger_args()));
    }

    run_status(Command::new("systemctl").args(systemd_systemctl_trigger_args()))
}

fn systemd_sudo_trigger_args() -> [String; 2] {
    ["-n".to_string(), systemd_upgrade_trigger_path()]
}

fn systemd_sudo_check_args(trigger_path: &str) -> Vec<String> {
    vec![
        "-n".to_string(),
        trigger_path.to_string(),
        "--check".to_string(),
    ]
}

fn systemd_sudo_list_start_args(trigger_path: &str) -> Vec<String> {
    vec!["-n".to_string(), "-l".to_string(), trigger_path.to_string()]
}

fn systemd_systemctl_trigger_args() -> [&'static str; 3] {
    ["start", "--no-block", SYSTEMD_UPGRADE_UNIT]
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
    fn active_status_reconciles_failed_systemd_delegate() {
        let tmp = tempdir().unwrap();
        let status = UpgradeJobStatus {
            state: UpgradeJobState::Running,
            target_tag: Some("v0.2.0".to_string()),
            repo: Some("IvanLi-CN/xp".to_string()),
            started_at: Some("2026-07-04T00:00:00Z".to_string()),
            finished_at: None,
            exit_code: None,
            message: Some("upgrade trigger accepted".to_string()),
            updated_at: "2026-07-04T00:00:00Z".to_string(),
        };
        write_status(tmp.path(), &status).unwrap();

        let loaded = read_status(tmp.path()).unwrap();
        let reconciled = reconcile_active_status(tmp.path(), loaded, || {
            Some(
                concat!(
                    "upgrade runner failed: xp-upgrade.service is failed ",
                    "(result=exit-code, exit_status=2)"
                )
                .to_string(),
            )
        })
        .unwrap();

        assert_eq!(reconciled.state, UpgradeJobState::Failed);
        assert_eq!(reconciled.target_tag.as_deref(), Some("v0.2.0"));
        assert!(reconciled.finished_at.is_some());
        assert!(
            reconciled
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("xp-upgrade.service is failed")
        );
        let persisted = read_status(tmp.path()).unwrap();
        assert_eq!(persisted.state, UpgradeJobState::Failed);
    }

    #[test]
    fn active_status_without_delegate_failure_stays_active() {
        let tmp = tempdir().unwrap();
        let status = UpgradeJobStatus {
            state: UpgradeJobState::Running,
            target_tag: Some("v0.2.0".to_string()),
            repo: Some("IvanLi-CN/xp".to_string()),
            started_at: Some("2026-07-04T00:00:00Z".to_string()),
            finished_at: None,
            exit_code: None,
            message: Some("upgrade trigger accepted".to_string()),
            updated_at: "2026-07-04T00:00:00Z".to_string(),
        };
        write_status(tmp.path(), &status).unwrap();

        let loaded = read_status(tmp.path()).unwrap();
        let reconciled = reconcile_active_status(tmp.path(), loaded, || None).unwrap();

        assert_eq!(reconciled.state, UpgradeJobState::Running);
        assert_eq!(reconciled.finished_at, None);
        assert_eq!(
            read_status(tmp.path()).unwrap().state,
            UpgradeJobState::Running
        );
    }

    #[test]
    fn parses_failed_systemd_unit_show_output() {
        let fields = parse_systemctl_show(concat!(
            "LoadState=loaded\n",
            "ActiveState=failed\n",
            "SubState=failed\n",
            "Result=exit-code\n",
            "ExecMainStatus=2\n",
        ));
        assert_eq!(fields.get("LoadState").map(String::as_str), Some("loaded"));
        assert_eq!(
            fields.get("ActiveState").map(String::as_str),
            Some("failed")
        );
        assert_eq!(fields.get("ExecMainStatus").map(String::as_str), Some("2"));
    }

    #[test]
    fn invalid_target_is_rejected() {
        let err = validate_target_tag("latest").unwrap_err();
        assert!(matches!(err, UpgradeStartError::InvalidTarget(_)));
    }

    #[test]
    fn delegate_asset_detection_requires_installed_runner_files() {
        let tmp = tempdir().unwrap();
        assert!(!systemd_upgrade_delegate_installed(tmp.path()));
        assert!(!openrc_upgrade_delegate_installed(tmp.path()));

        let systemd_unit = root_abs(tmp.path(), SYSTEMD_UPGRADE_UNIT_PATH);
        fs::create_dir_all(systemd_unit.parent().unwrap()).unwrap();
        fs::write(systemd_unit, "unit").unwrap();
        assert!(!systemd_upgrade_delegate_installed(tmp.path()));

        let systemd_helper = root_abs(tmp.path(), SYSTEMD_UPGRADE_TRIGGER_PATH);
        let systemd_sudoers = root_abs(tmp.path(), SYSTEMD_SUDOERS_PATH);
        fs::create_dir_all(systemd_helper.parent().unwrap()).unwrap();
        fs::create_dir_all(systemd_sudoers.parent().unwrap()).unwrap();
        fs::write(systemd_helper, "helper").unwrap();
        fs::write(&systemd_sudoers, SYSTEMD_UPGRADE_SUDOERS_CHECK_RULE).unwrap();
        assert!(!systemd_upgrade_delegate_installed(tmp.path()));
        fs::write(&systemd_sudoers, SYSTEMD_UPGRADE_SUDOERS_START_RULE).unwrap();
        assert!(!systemd_upgrade_delegate_installed(tmp.path()));
        fs::write(
            &systemd_sudoers,
            format!(
                "{SYSTEMD_UPGRADE_SUDOERS_START_RULE}\n\
                 {SYSTEMD_UPGRADE_SUDOERS_CHECK_RULE}\n"
            ),
        )
        .unwrap();
        assert!(systemd_upgrade_delegate_installed(tmp.path()));

        let tmp = tempdir().unwrap();
        let systemd_unit = root_abs(tmp.path(), SYSTEMD_UPGRADE_UNIT_PATH);
        fs::create_dir_all(systemd_unit.parent().unwrap()).unwrap();
        fs::write(systemd_unit, "unit").unwrap();
        let systemd_polkit = root_abs(tmp.path(), SYSTEMD_POLKIT_RULE_PATH);
        fs::create_dir_all(systemd_polkit.parent().unwrap()).unwrap();
        fs::write(
            systemd_polkit,
            format!(
                "{SYSTEMD_POLKIT_ACTION}\n\
                 {SYSTEMD_UPGRADE_POLKIT_UNIT_RULE}\n\
                 {SYSTEMD_UPGRADE_POLKIT_VERB_RULE}\n"
            ),
        )
        .unwrap();
        assert!(systemd_upgrade_delegate_installed(tmp.path()));
        assert!(!systemd_upgrade_delegate_installed_with_polkit_file(
            tmp.path(),
            false
        ));

        let openrc_script = root_abs(tmp.path(), OPENRC_UPGRADE_SCRIPT_PATH);
        let doas_conf = root_abs(tmp.path(), OPENRC_DOAS_CONF_PATH);
        fs::create_dir_all(openrc_script.parent().unwrap()).unwrap();
        fs::create_dir_all(doas_conf.parent().unwrap()).unwrap();
        fs::write(openrc_script, "script").unwrap();
        fs::write(doas_conf, OPENRC_UPGRADE_DOAS_RULE).unwrap();
        assert!(openrc_upgrade_delegate_installed(tmp.path()));
    }

    #[test]
    fn runner_rejects_tampered_target_tag_and_records_failure() {
        let tmp = tempdir().unwrap();
        let request = UpgradeRequest {
            target_tag: "latest".to_string(),
            repo: Some("IvanLi-CN/xp".to_string()),
            requested_at: "2026-07-04T00:00:00Z".to_string(),
        };

        write_request(tmp.path(), &request).unwrap();
        let err = prepare_runner_request(tmp.path(), "IvanLi-CN/xp").unwrap_err();
        assert_eq!(err.code, 3);
        assert!(
            err.message
                .contains("target_tag must be a v-prefixed release tag")
        );

        let status = read_status(tmp.path()).unwrap();
        assert_eq!(status.state, UpgradeJobState::Failed);
        assert_eq!(status.target_tag.as_deref(), Some("latest"));
        assert_eq!(status.exit_code, Some(3));
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

    #[test]
    fn systemd_trigger_uses_fixed_sudo_helper_or_fixed_unit() {
        let helper = "/usr/local/libexec/xp-upgrade-trigger";
        assert_eq!(
            systemd_sudo_check_args(helper),
            [
                "-n".to_string(),
                "/usr/local/libexec/xp-upgrade-trigger".to_string(),
                "--check".to_string()
            ]
        );
        assert_eq!(
            systemd_sudo_list_start_args(helper),
            [
                "-n".to_string(),
                "-l".to_string(),
                "/usr/local/libexec/xp-upgrade-trigger".to_string()
            ]
        );
        assert_eq!(
            systemd_sudo_trigger_args(),
            [
                "-n".to_string(),
                "/usr/local/libexec/xp-upgrade-trigger".to_string()
            ]
        );
        assert_eq!(
            systemd_systemctl_trigger_args(),
            ["start", "--no-block", "xp-upgrade.service"]
        );
    }
}
