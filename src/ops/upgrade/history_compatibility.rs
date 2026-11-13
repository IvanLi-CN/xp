use super::failure::unrestored_transaction_backup_error;
use crate::ops::cli::ExitError;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::Path;

const MIN_READER: (u64, u64, u64) = (3, 47, 9);
pub(super) const ROLLBACK_BLOCKED: &str = "history_reader_rollback_blocked";

fn supports_recovery(raw: &str) -> bool {
    let raw = raw.trim().strip_prefix('v').unwrap_or(raw.trim());
    let core = raw.split(['-', '+']).next().unwrap_or("");
    let parts = core
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>();
    let Ok(parts) = parts else { return false };
    let [major, minor, patch] = parts.as_slice() else {
        return false;
    };
    let version = (*major, *minor, *patch);
    version > MIN_READER
        || (version == MIN_READER && !raw.split('+').next().unwrap_or(raw).contains('-'))
}

fn has_recovery(data_dir: &Path) -> Result<bool, ExitError> {
    let database = data_dir.join("history.sqlite3");
    match database.try_exists() {
        Ok(false) => return Ok(false),
        Ok(true) => {}
        Err(error) => return Err(storage_error(error)),
    }
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(storage_error)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(storage_error)?;
    let value = connection
        .query_row(
            "SELECT json_type(payload, '$.prefix_recoveries'),
                json_array_length(payload, '$.prefix_recoveries')
         FROM history_snapshots WHERE key = 'repository_replica'",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    match value {
        None | Some((None, _)) => Ok(false),
        Some((Some(kind), Some(count))) if kind == "array" => Ok(count > 0),
        _ => Err(storage_error("invalid prefix recovery ledger")),
    }
}

fn storage_error(error: impl std::fmt::Display) -> ExitError {
    ExitError::new(7, format!("history_reader_compatibility: {error}"))
}

pub(super) fn ensure_target(data_dir: &Path, tag: &str) -> Result<(), ExitError> {
    if supports_recovery(tag) || !has_recovery(data_dir)? {
        return Ok(());
    }
    Err(ExitError::new(
        3,
        "history_reader_compatibility: retained prefix recovery requires XP >= 3.47.9",
    ))
}

pub(super) fn ensure_rollback(data_dir: &Path, backup: &Path) -> Result<(), ExitError> {
    let check = || -> Result<(), ExitError> {
        if !has_recovery(data_dir)? {
            return Ok(());
        }
        let output = std::process::Command::new(backup)
            .arg("--version")
            .output()
            .map_err(storage_error)?;
        let text = std::str::from_utf8(&output.stdout).map_err(storage_error)?;
        let tag = text
            .trim()
            .strip_prefix("xp ")
            .ok_or_else(|| storage_error("invalid backup version"))?;
        if !output.status.success() || !supports_recovery(tag) {
            return Err(storage_error(
                "backup cannot retain prefix recovery evidence",
            ));
        }
        Ok(())
    };
    check().map_err(|error| {
        unrestored_transaction_backup_error(format!("{ROLLBACK_BLOCKED}: {}", error.message))
    })
}

#[cfg(test)]
#[path = "history_compatibility_tests.rs"]
mod tests;
