use super::{ExitError, GuardConfig, GuardProfile, GuardStatus, Mode, OWNERSHIP_MARKER, SCHEMA};
use crate::ops::paths::Paths;
use crate::ops::platform::InitSystem;
use crate::ops::util::{chmod, ensure_dir, is_test_root};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn write_config(paths: &Paths, config: &GuardConfig) -> Result<(), super::ExitError> {
    reject_symlink(&paths.etc_xp_ops_dir())?;
    ensure_dir(&paths.etc_xp_ops_dir()).map_err(fs_error)?;
    let path = paths.etc_xp_ops_ingress_guard_config();
    reject_symlink(&path)?;
    let content = toml::to_string_pretty(config).map_err(|error| {
        super::ExitError::new(4, format!("config serialization failed: {error}"))
    })?;
    atomic_write(&path, content.as_bytes(), 0o600)
}

pub(super) fn load_config(paths: &Paths) -> Result<Option<GuardConfig>, super::ExitError> {
    let path = paths.etc_xp_ops_ingress_guard_config();
    reject_symlink(&path)?;
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(fs_error(error)),
    };
    let config = toml::from_str(&raw).map_err(|error| {
        super::ExitError::new(4, format!("invalid ingress guard config: {error}"))
    })?;
    validate_config(&config)?;
    Ok(Some(config))
}

pub(super) fn write_status(paths: &Paths, status: GuardStatus) -> Result<(), super::ExitError> {
    ensure_guard_runtime_dir(paths)?;
    let content = serde_json::to_vec_pretty(&status).map_err(|error| {
        super::ExitError::new(4, format!("status serialization failed: {error}"))
    })?;
    atomic_write(&paths.run_xp_ingress_guard_status(), &content, 0o644)
}

pub(super) fn write_permit(paths: &Paths, config: &GuardConfig) -> Result<(), super::ExitError> {
    ensure_guard_runtime_dir(paths)?;
    atomic_write(
        &paths.run_xp_ingress_guard_permit(),
        super::permit_token(config).as_bytes(),
        0o644,
    )
}

pub(super) fn remove_permit(paths: &Paths) -> Result<(), super::ExitError> {
    ensure_guard_runtime_dir(paths)?;
    let path = paths.run_xp_ingress_guard_permit();
    reject_symlink(&path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(fs_error(error)),
    }
}

pub(super) fn ensure_guard_runtime_dir(paths: &Paths) -> Result<(), super::ExitError> {
    let dir = paths.run_xp_ingress_guard_dir();
    reject_symlink(&dir)?;
    ensure_dir(&dir).map_err(fs_error)?;
    reject_symlink(&dir)?;
    #[cfg(unix)]
    if !is_test_root(paths.root()) {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(&dir).map_err(fs_error)?;
        if metadata.uid() != 0 {
            return Err(super::ExitError::new(
                6,
                "filesystem_error: ingress guard runtime directory must be root-owned",
            ));
        }
    }
    chmod(&dir, 0o755).map_err(fs_error)
}

fn atomic_write(path: &Path, contents: &[u8], mode: u32) -> Result<(), super::ExitError> {
    if let Some(parent) = path.parent() {
        reject_symlink(parent)?;
        ensure_dir(parent).map_err(fs_error)?;
    }
    reject_symlink(path)?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    reject_symlink(&temp)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(fs_error)?;
    file.write_all(contents).map_err(fs_error)?;
    file.sync_all().map_err(fs_error)?;
    chmod(&temp, mode).map_err(fs_error)?;
    fs::rename(&temp, path).map_err(fs_error)
}

pub(super) fn reject_symlink(path: &Path) -> Result<(), super::ExitError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(super::ExitError::new(
            6,
            format!(
                "filesystem_error: symlink state path rejected: {}",
                path.display()
            ),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(fs_error(error)),
    }
}

pub(super) fn validate_config(config: &GuardConfig) -> Result<(), super::ExitError> {
    if config.schema != SCHEMA || config.ownership != OWNERSHIP_MARKER {
        return Err(super::ExitError::new(
            3,
            "ownership_conflict: invalid guard ownership marker",
        ));
    }
    validate_limits(
        config.global_rate,
        config.global_burst,
        config.source_rate,
        config.source_burst,
    )?;
    if config.profile != GuardProfile::SmallVps.as_str() && config.profile != "custom" {
        return Err(super::ExitError::new(2, "unsupported_profile"));
    }
    if config.cgroup.is_empty()
        || config.cgroup.contains('"')
        || config.cgroup.contains('\\')
        || config.cgroup.contains('\n')
    {
        return Err(super::ExitError::new(
            2,
            "cgroup_read_failed: invalid cgroup path",
        ));
    }
    Ok(())
}

pub(super) fn validate_limits(
    global_rate: u32,
    global_burst: u32,
    source_rate: u32,
    source_burst: u32,
) -> Result<(), super::ExitError> {
    if [global_rate, global_burst, source_rate, source_burst]
        .into_iter()
        .any(|value| value == 0 || value > super::MAX_LIMIT)
    {
        return Err(super::ExitError::new(
            2,
            "invalid_limits: values must be between 1 and 1000000",
        ));
    }
    Ok(())
}

pub(super) fn fs_error(error: impl std::fmt::Display) -> super::ExitError {
    super::ExitError::new(4, format!("filesystem_error: {error}"))
}

pub(super) fn running_as_root(paths: &Paths) -> bool {
    if is_test_root(paths.root()) {
        return true;
    }
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

impl Mode {
    pub(super) fn from_dry_run(dry_run: bool) -> Self {
        if dry_run { Self::DryRun } else { Self::Real }
    }
}

struct TableReadback {
    json: Value,
}

fn read_table_json(paths: &Paths) -> Result<TableReadback, super::ExitError> {
    if is_test_root(paths.root()) && std::env::var_os("XP_OPS_NFT_BIN").is_none() {
        return Ok(TableReadback { json: Value::Null });
    }
    let output = Command::new(super::nft::binary())
        .args(["--json", "list", "table", "inet", super::TABLE_NAME])
        .output()
        .map_err(|error| super::ExitError::new(3, format!("nft_readback_failed: {error}")))?;
    if !output.status.success() {
        return Err(super::ExitError::new(
            3,
            "nft_readback_failed: table is not readable",
        ));
    }
    let json = serde_json::from_slice(&output.stdout).map_err(|error| {
        super::ExitError::new(3, format!("nft_readback_failed: invalid JSON: {error}"))
    })?;
    Ok(TableReadback { json })
}

pub(super) fn verify_readback(paths: &Paths, config: &GuardConfig) -> Result<(), super::ExitError> {
    let readback = read_table_json(paths)?;
    if is_test_root(paths.root()) && std::env::var_os("XP_OPS_NFT_BIN").is_none() {
        return Ok(());
    }
    let text = readback.json.to_string();
    let table = readback
        .json
        .get("nftables")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                let table = item.get("table")?;
                (table.get("family").and_then(Value::as_str) == Some("inet")
                    && table.get("name").and_then(Value::as_str) == Some(super::TABLE_NAME))
                .then_some(table)
            })
        })
        .ok_or_else(|| super::ExitError::new(3, "nft_readback_failed: table identity mismatch"))?;
    if table.get("comment").and_then(Value::as_str) != Some(super::OWNERSHIP_MARKER) {
        return Err(super::ExitError::new(
            3,
            "nft_readback_failed: table ownership marker mismatch",
        ));
    }
    for marker in [
        "global_over_limit",
        "source_v4_over_limit",
        "source_v6_over_limit",
        "admitted_syns",
        &config.cgroup,
        "socket",
        "iifname",
        "tcp",
        "flags",
        "payload",
        "lo",
        "input",
        "-300",
        "1024",
        "60s",
    ] {
        if !text.contains(marker) {
            return Err(super::ExitError::new(
                3,
                format!("nft_readback_failed: missing expected rule detail {marker}"),
            ));
        }
    }
    for (field, value) in [
        ("rate", config.global_rate),
        ("burst", config.global_burst),
        ("rate", config.source_rate),
        ("burst", config.source_burst),
    ] {
        let json_value = format!("\"{field}\":{value}");
        let json_value_spaced = format!("\"{field}\": {value}");
        if !text.contains(&json_value) && !text.contains(&json_value_spaced) {
            return Err(super::ExitError::new(
                3,
                format!("nft_readback_failed: {field} value {value} missing"),
            ));
        }
    }
    let verdict = if config.mode == super::GuardMode::Enforced {
        "drop"
    } else {
        "return"
    };
    if !text.contains(verdict) {
        return Err(super::ExitError::new(
            3,
            format!("nft_readback_failed: expected {verdict} verdict missing"),
        ));
    }
    Ok(())
}

pub(super) fn update_status_counters(
    paths: &Paths,
    status: &mut GuardStatus,
) -> Result<(), super::ExitError> {
    let readback = read_table_json(paths)?;
    let mut counters = [0_u64; 3];
    collect_counters(&readback.json, &mut counters);
    status.global_over_limit = counters[0];
    status.source_v4_over_limit = counters[1];
    status.source_v6_over_limit = counters[2];
    Ok(())
}

fn collect_counters(value: &Value, counters: &mut [u64; 3]) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_counters(item, counters);
            }
        }
        Value::Object(object) => {
            if let Some(Value::Object(counter)) = object.get("counter")
                && let (Some(Value::String(name)), Some(Value::Number(packets))) =
                    (counter.get("name"), counter.get("packets"))
                && let Some(packets) = packets.as_u64()
            {
                match name.as_str() {
                    "global_over_limit" => counters[0] = packets,
                    "source_v4_over_limit" => counters[1] = packets,
                    "source_v6_over_limit" => counters[2] = packets,
                    _ => {}
                }
            }
            for child in object.values() {
                collect_counters(child, counters);
            }
        }
        _ => {}
    }
}

pub(super) fn validate_xray_service_asset(
    paths: &Paths,
    init: InitSystem,
) -> Result<(), super::ExitError> {
    let (path, contents) = match init {
        InitSystem::Systemd => (
            paths.systemd_unit_dir().join("xray.service"),
            fs::read_to_string(paths.systemd_unit_dir().join("xray.service")),
        ),
        InitSystem::OpenRc => (
            paths.openrc_initd_dir().join("xray"),
            fs::read_to_string(paths.openrc_initd_dir().join("xray")),
        ),
        InitSystem::None => return Err(super::ExitError::new(2, "unsupported_service")),
    };
    let raw = contents.map_err(|_| {
        super::ExitError::new(
            2,
            format!(
                "unsupported_service: managed Xray asset missing ({})",
                path.display()
            ),
        )
    })?;
    let standard = match init {
        InitSystem::Systemd => {
            raw.contains("User=xray")
                && raw.contains("Group=xray")
                && (raw.contains("ExecStart=/usr/local/bin/xray run -c /etc/xray/config.json")
                    || raw.contains("ExecStart=/usr/local/bin/xp-ops _ingress-guard-exec"))
        }
        InitSystem::OpenRc => {
            raw.contains("command_user=\"xray:xray\"")
                && raw.contains("supervisor=supervise-daemon")
                && (raw.contains("command=\"/usr/local/bin/xray\"")
                    || raw.contains("command=\"/usr/local/bin/xp-ops\""))
                && (raw.contains("/etc/xray/config.json") || raw.contains("_ingress-guard-exec"))
        }
        InitSystem::None => false,
    };
    let legacy_managed = match init {
        InitSystem::Systemd => {
            raw.contains("Description=xray (local proxy runtime)")
                && raw.contains("Wants=network-online.target")
                && raw.contains("After=network-online.target")
                && raw.contains("Environment=GOMEMLIMIT=16MiB")
                && raw.contains("Environment=GOGC=50")
                && raw.contains("Restart=always")
        }
        InitSystem::OpenRc => {
            raw.contains("description=\"xray (local proxy runtime)\"")
                && raw.contains("depend()")
                && raw.contains("need net")
        }
        InitSystem::None => false,
    };
    if !standard
        || (!raw.contains("# Managed by xp-ops ingress-guard service boundary") && !legacy_managed)
    {
        return Err(ExitError::new(
            2,
            "unsupported_service: custom Xray service asset",
        ));
    }
    Ok(())
}

pub(super) fn current_xray_cgroup(paths: &Paths) -> Result<String, super::ExitError> {
    if let Ok(value) = std::env::var("XP_OPS_TEST_CGROUP") {
        return normalize_cgroup(&value);
    }
    let path = if is_test_root(paths.root()) {
        PathBuf::from("/proc/self/cgroup")
    } else {
        paths.map_abs(Path::new("/proc/self/cgroup"))
    };
    let raw = fs::read_to_string(path)
        .map_err(|error| ExitError::new(2, format!("cgroup_read_failed: {error}")))?;
    let value = raw
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or_else(|| ExitError::new(2, "unsupported_kernel: unified cgroup path missing"))?;
    normalize_cgroup(value)
}

pub(super) fn xray_service_cgroup(paths: &Paths) -> Result<String, super::ExitError> {
    if std::env::var_os("XP_OPS_TEST_CGROUP").is_some() {
        return current_xray_cgroup(paths);
    }
    let proc_dir = paths.map_abs(Path::new("/proc"));
    if let Ok(entries) = fs::read_dir(proc_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name
                .to_str()
                .is_none_or(|value| !value.chars().all(char::is_numeric))
            {
                continue;
            }
            if fs::read_to_string(entry.path().join("comm"))
                .ok()
                .as_deref()
                .map(str::trim)
                != Some("xray")
            {
                continue;
            }
            if let Ok(raw) = fs::read_to_string(entry.path().join("cgroup"))
                && let Some(value) = raw.lines().find_map(|line| line.strip_prefix("0::"))
            {
                return normalize_cgroup(value);
            }
        }
    }
    current_xray_cgroup(paths)
}

pub(super) fn normalize_cgroup(value: &str) -> Result<String, super::ExitError> {
    let value = value.trim();
    if value.is_empty()
        || value.contains('\0')
        || value.contains('\"')
        || value.contains('\\')
        || value.contains('\n')
    {
        return Err(ExitError::new(2, "cgroup_read_failed: invalid cgroup path"));
    }
    Ok(value.trim_start_matches('/').to_string())
}

pub(super) fn validate_owned_table(paths: &Paths) -> Result<(), super::ExitError> {
    let Some(table) = read_table_value(paths)? else {
        return Ok(());
    };
    let comment = table
        .get("comment")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if comment != OWNERSHIP_MARKER {
        return Err(super::ExitError::new(
            3,
            "ownership_conflict: foreign nft table uses xp_ingress_guard",
        ));
    }
    Ok(())
}

pub(super) fn remove_owned_table(paths: &Paths) -> Result<bool, super::ExitError> {
    if read_table_value(paths)?.is_none() {
        return Ok(true);
    }
    validate_owned_table(paths)?;
    let status = Command::new(super::nft::binary())
        .args(["delete", "table", "inet", super::TABLE_NAME])
        .status()
        .map_err(|error| super::ExitError::new(3, format!("nft_failed: {error}")))?;
    Ok(status.success())
}

pub(super) fn read_table_value(paths: &Paths) -> Result<Option<Value>, super::ExitError> {
    if is_test_root(paths.root()) && std::env::var_os("XP_OPS_NFT_BIN").is_none() {
        return Ok(None);
    }
    let output = Command::new(super::nft::binary())
        .args(["--json", "list", "table", "inet", super::TABLE_NAME])
        .output()
        .map_err(|error| super::ExitError::new(3, format!("nft_readback_failed: {error}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    let json: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        super::ExitError::new(3, format!("nft_readback_failed: invalid JSON: {error}"))
    })?;
    Ok(json
        .get("nftables")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                let table = item.get("table")?;
                (table.get("family").and_then(Value::as_str) == Some("inet")
                    && table.get("name").and_then(Value::as_str) == Some(super::TABLE_NAME))
                .then(|| table.clone())
            })
        }))
}
