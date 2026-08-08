use crate::ops::Paths;
use crate::ops::cli::ExitError;
use crate::ops::runtime_activation::restart_xp_service;
use crate::ops::upgrade_artifacts::{cleanup_managed_artifacts_for, ensure_upgrade_space_for};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn rollback_xp_ops_after_resumed_failure(
    dest: &Path,
    backup: &Path,
    original_err: ExitError,
) -> Result<(), ExitError> {
    let failed = dest.with_extension(format!("failed.{}", super::now_unix_secs()));
    if dest.exists() {
        fs::rename(dest, &failed).map_err(|error| {
            ExitError::new(
                8,
                format!(
                    "rollback_failed: stash upgraded xp-ops after resumed failure: {error}; \
                     original error: {}",
                    original_err.message
                ),
            )
        })?;
    }
    fs::rename(backup, dest).map_err(|error| {
        ExitError::new(
            8,
            format!(
                "rollback_failed: restore xp-ops after resumed failure: {error}; \
                 original error: {}",
                original_err.message
            ),
        )
    })?;
    let _ = fs::remove_file(&failed);
    Err(ExitError::new(
        original_err.code,
        format!("{}; rolled back xp-ops", original_err.message),
    ))
}

pub(super) fn rollback_xp_after_xray_failure(
    paths: &Paths,
    backup: &Path,
    original_err: ExitError,
) -> Result<(), ExitError> {
    let dest = paths.usr_local_bin_xp();
    let failed = dest.with_extension(format!("failed.{}", super::now_unix_secs()));
    if dest.exists() {
        fs::rename(&dest, &failed).map_err(|error| {
            ExitError::new(
                8,
                format!(
                    "rollback_failed: stash upgraded xp after xray failure: {error}; \
                     original error: {}",
                    original_err.message
                ),
            )
        })?;
    }
    fs::rename(backup, &dest).map_err(|error| {
        ExitError::new(
            8,
            format!(
                "rollback_failed: restore xp after xray failure: {error}; original error: {}",
                original_err.message
            ),
        )
    })?;
    let _ = fs::remove_file(&failed);
    if !restart_xp_service(paths) {
        return Err(ExitError::new(
            8,
            format!(
                concat!(
                    "rollback_failed: xp rollback restart failed after xray failure; ",
                    "original error: {}"
                ),
                original_err.message
            ),
        ));
    }
    Err(ExitError::new(
        original_err.code,
        format!("{}; rolled back xp", original_err.message),
    ))
}

pub(super) fn clear_upgrade_diagnostics(data_dir: &Path) {
    let path = diagnostics_path(data_dir);
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(path.with_extension("json.tmp"));
}

pub(super) fn record_early_upgrade_failure(
    paths: &Paths,
    data_dir: &Path,
    release_tag: &str,
    checksums: &HashMap<String, [u8; 32]>,
    error: ExitError,
) -> ExitError {
    let error = match cleanup_managed_artifacts_for(paths, &[]) {
        Ok(_) => error,
        Err(cleanup_error) => ExitError::new(
            7,
            format!(
                concat!(
                    "service_error: cleanup failed upgrade artifacts: ",
                    "{}; original error: {}"
                ),
                cleanup_error, error.message
            ),
        ),
    };
    write_upgrade_diagnostics(data_dir, release_tag, checksums, &error);
    error
}

pub(super) fn preflight_upgrade(
    paths: &Paths,
    xp_dest: &Path,
    xp_ops_dest: &Path,
) -> Result<(), ExitError> {
    if !xp_dest.exists() {
        return Err(ExitError::new(3, "invalid_args: xp is not installed"));
    }
    cleanup_managed_artifacts_for(paths, &[xp_ops_dest]).map_err(|error| {
        ExitError::new(
            7,
            format!("service_error: cleanup upgrade artifacts: {error}"),
        )
    })?;
    ensure_upgrade_space_for(paths, &[xp_ops_dest]).map_err(|message| ExitError::new(3, message))
}

pub(super) fn write_upgrade_diagnostics(
    data_dir: &Path,
    release_tag: &str,
    checksums: &HashMap<String, [u8; 32]>,
    error: &ExitError,
) {
    let mut assets = checksums
        .iter()
        .filter(|(name, _)| {
            ["xp-", "xp-ops-", "xray-", "cloudflared-"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
        .map(|(name, checksum)| {
            (
                name.chars().take(256).collect::<String>(),
                hex::encode(checksum),
            )
        })
        .collect::<BTreeMap<_, _>>();
    while assets.len() > 4 {
        let Some(name) = assets.last_key_value().map(|(name, _)| name.clone()) else {
            break;
        };
        assets.remove(&name);
    }
    let value = serde_json::json!({
        "release_tag": release_tag.chars().take(256).collect::<String>(),
        "assets_sha256": assets,
        "failure_phase": "upgrade",
        "exit_code": error.code,
        "error_summary": error.message.chars().take(1_024).collect::<String>(),
    });
    let Ok(mut raw) = serde_json::to_vec_pretty(&value) else {
        return;
    };
    if raw.len().saturating_add(1) > 8 * 1024 {
        let fallback = serde_json::json!({
            "release_tag": release_tag.chars().take(256).collect::<String>(),
            "assets_sha256": {},
            "failure_phase": "upgrade",
            "exit_code": error.code,
            "error_summary": error.message.chars().take(256).collect::<String>(),
        });
        let Ok(fallback) = serde_json::to_vec_pretty(&fallback) else {
            return;
        };
        raw = fallback;
    }
    raw.push(b'\n');
    let dir = crate::upgrade_job::upgrade_dir(data_dir);
    if fs::create_dir_all(&dir).is_ok() {
        let tmp = diagnostics_path(data_dir).with_extension("json.tmp");
        if fs::write(&tmp, raw).is_ok() {
            if fs::rename(&tmp, diagnostics_path(data_dir)).is_err() {
                let _ = fs::remove_file(tmp);
            }
        } else {
            let _ = fs::remove_file(tmp);
        }
    }
}

fn diagnostics_path(data_dir: &Path) -> PathBuf {
    crate::upgrade_job::upgrade_dir(data_dir).join("diagnostics.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn diagnostics_retain_only_current_failure_within_size_limit() {
        let tmp = tempdir().unwrap();
        let mut checksums = HashMap::new();
        for index in 0..16 {
            checksums.insert(
                format!("xp-{}-{}", "x".repeat(400), index),
                [index as u8; 32],
            );
        }
        let error = ExitError::new(7, format!("service_error: {}", "x".repeat(4_096)));
        write_upgrade_diagnostics(tmp.path(), "v0.2.0", &checksums, &error);

        let path = diagnostics_path(tmp.path());
        let raw = fs::read(&path).unwrap();
        assert!(raw.len() <= 8 * 1024);
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["release_tag"], "v0.2.0");
        assert_eq!(value["exit_code"], 7);
        assert!(value["assets_sha256"].as_object().unwrap().len() <= 4);

        write_upgrade_diagnostics(
            tmp.path(),
            "v0.2.1",
            &HashMap::new(),
            &ExitError::new(5, "download_failed: unavailable"),
        );
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["release_tag"], "v0.2.1");
        assert_eq!(value["assets_sha256"], serde_json::json!({}));

        clear_upgrade_diagnostics(tmp.path());
        assert!(!path.exists());
    }

    #[test]
    fn diagnostics_temp_is_removed_after_rename_failure() {
        let tmp = tempdir().unwrap();
        let path = diagnostics_path(tmp.path());
        fs::create_dir_all(&path).unwrap();

        write_upgrade_diagnostics(
            tmp.path(),
            "v0.2.0",
            &HashMap::new(),
            &ExitError::new(5, "download_failed"),
        );

        assert!(!path.with_extension("json.tmp").exists());
    }
}
