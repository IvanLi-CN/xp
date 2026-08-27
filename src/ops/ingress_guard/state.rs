use super::{GuardConfig, GuardProfile, GuardStatus, Mode, OWNERSHIP_MARKER, SCHEMA};
use crate::ops::paths::Paths;
use crate::ops::util::{chmod, ensure_dir, is_test_root};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
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
    reject_symlink(&paths.run_xp_ingress_guard_dir())?;
    ensure_dir(&paths.run_xp_ingress_guard_dir()).map_err(fs_error)?;
    let content = serde_json::to_vec_pretty(&status).map_err(|error| {
        super::ExitError::new(4, format!("status serialization failed: {error}"))
    })?;
    atomic_write(&paths.run_xp_ingress_guard_status(), &content, 0o644)
}

pub(super) fn write_permit(paths: &Paths, config: &GuardConfig) -> Result<(), super::ExitError> {
    reject_symlink(&paths.run_xp_ingress_guard_dir())?;
    ensure_dir(&paths.run_xp_ingress_guard_dir()).map_err(fs_error)?;
    atomic_write(
        &paths.run_xp_ingress_guard_permit(),
        super::permit_token(config).as_bytes(),
        0o644,
    )
}

pub(super) fn remove_permit(paths: &Paths) -> Result<(), super::ExitError> {
    let path = paths.run_xp_ingress_guard_permit();
    reject_symlink(&path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(fs_error(error)),
    }
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
    for marker in [
        super::TABLE_NAME,
        super::OWNERSHIP_MARKER,
        "global_over_limit",
        "source_v4_over_limit",
        "source_v6_over_limit",
    ] {
        if !text.contains(marker) {
            return Err(super::ExitError::new(
                3,
                format!("nft_readback_failed: missing owned marker {marker}"),
            ));
        }
    }
    if !text.contains(&config.cgroup) {
        return Err(super::ExitError::new(
            3,
            "nft_readback_failed: cgroup selector mismatch",
        ));
    }
    Ok(())
}

pub(super) fn update_status_counters(paths: &Paths, status: &mut GuardStatus) {
    let Ok(readback) = read_table_json(paths) else {
        return;
    };
    let mut counters = [0_u64; 3];
    collect_counters(&readback.json, &mut counters);
    status.global_over_limit = counters[0];
    status.source_v4_over_limit = counters[1];
    status.source_v6_over_limit = counters[2];
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
