use serde::{Deserialize, Serialize};

use super::*;

pub(crate) const DELIVERY_RATE_WINDOW_SECONDS: u64 = 5 * 60;

fn default_delivery_rate_window_seconds() -> u64 {
    DELIVERY_RATE_WINDOW_SECONDS
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceDeliveryAttempt {
    pub(crate) now_unix_seconds: u64,
    pub(crate) attempts: u64,
    pub(crate) successes: u64,
    pub(crate) outcome: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepositoryDeliveryMetrics {
    #[serde(default)]
    pub(crate) generated_segments: u64,
    #[serde(default)]
    pub(crate) generated_bytes: u64,
    #[serde(default)]
    pub(crate) acknowledged_segments: u64,
    #[serde(default)]
    pub(crate) acknowledged_bytes: u64,
    #[serde(default)]
    pub(crate) applied_segments: u64,
    #[serde(default)]
    pub(crate) applied_bytes: u64,
    #[serde(default)]
    pub(crate) source_delivery_attempts: u64,
    #[serde(default)]
    pub(crate) source_delivery_successes: u64,
    #[serde(default)]
    pub(crate) source_transport_errors: u64,
    #[serde(default)]
    pub(crate) source_response_errors: u64,
    #[serde(default)]
    pub(crate) source_response_changed: u64,
    #[serde(default)]
    pub(crate) source_explicit_unavailable: u64,
    #[serde(default)]
    pub(crate) source_storage_errors: u64,
    #[serde(default)]
    pub(crate) last_attempt_at: Option<u64>,
    #[serde(default)]
    pub(crate) last_progress_at: Option<u64>,
    #[serde(default)]
    pub(crate) counter_epoch: u64,
    #[serde(default = "default_delivery_rate_window_seconds")]
    pub(crate) rate_window_seconds: u64,
}

/// A bounded observation derived from two durable metric snapshots. Keeping this calculation
/// separate from the persisted counters makes the five-minute denominator explicit and prevents
/// queue-depth deltas from being mistaken for processing capacity.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RepositoryDeliveryWindow {
    pub(crate) elapsed_seconds: u64,
    pub(crate) generated_segments: u64,
    pub(crate) generated_bytes: u64,
    pub(crate) acknowledged_segments: u64,
    pub(crate) acknowledged_bytes: u64,
    pub(crate) applied_segments: u64,
    pub(crate) applied_bytes: u64,
}

#[cfg(test)]
impl RepositoryDeliveryWindow {
    pub(crate) fn source_ack_meets_ten_x(&self) -> bool {
        self.elapsed_seconds >= DELIVERY_RATE_WINDOW_SECONDS
            && self.generated_segments > 0
            && self.generated_bytes > 0
            && self.acknowledged_segments >= self.generated_segments.saturating_mul(10)
            && self.acknowledged_bytes >= self.generated_bytes.saturating_mul(10)
    }

    pub(crate) fn repository_apply_meets_ten_x(&self) -> bool {
        self.elapsed_seconds >= DELIVERY_RATE_WINDOW_SECONDS
            && self.generated_segments > 0
            && self.generated_bytes > 0
            && self.applied_segments >= self.generated_segments.saturating_mul(10)
            && self.applied_bytes >= self.generated_bytes.saturating_mul(10)
    }
}

impl Default for RepositoryDeliveryMetrics {
    fn default() -> Self {
        Self {
            generated_segments: 0,
            generated_bytes: 0,
            acknowledged_segments: 0,
            acknowledged_bytes: 0,
            applied_segments: 0,
            applied_bytes: 0,
            source_delivery_attempts: 0,
            source_delivery_successes: 0,
            source_transport_errors: 0,
            source_response_errors: 0,
            source_response_changed: 0,
            source_explicit_unavailable: 0,
            source_storage_errors: 0,
            last_attempt_at: None,
            last_progress_at: None,
            counter_epoch: 0,
            rate_window_seconds: DELIVERY_RATE_WINDOW_SECONDS,
        }
    }
}

impl RepositoryDeliveryMetrics {
    #[cfg(test)]
    pub(crate) fn window_since(
        &self,
        start: &Self,
        elapsed_seconds: u64,
    ) -> Option<RepositoryDeliveryWindow> {
        if elapsed_seconds == 0
            || self.generated_segments < start.generated_segments
            || self.generated_bytes < start.generated_bytes
            || self.acknowledged_segments < start.acknowledged_segments
            || self.acknowledged_bytes < start.acknowledged_bytes
            || self.applied_segments < start.applied_segments
            || self.applied_bytes < start.applied_bytes
        {
            return None;
        }
        Some(RepositoryDeliveryWindow {
            elapsed_seconds,
            generated_segments: self.generated_segments - start.generated_segments,
            generated_bytes: self.generated_bytes - start.generated_bytes,
            acknowledged_segments: self.acknowledged_segments - start.acknowledged_segments,
            acknowledged_bytes: self.acknowledged_bytes - start.acknowledged_bytes,
            applied_segments: self.applied_segments - start.applied_segments,
            applied_bytes: self.applied_bytes - start.applied_bytes,
        })
    }
}

pub(super) fn ensure_repository_delivery_metrics(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction().map_err(sqlite_error)?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS repository_delivery_metrics (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 generated_segments INTEGER NOT NULL DEFAULT 0,
                 generated_bytes INTEGER NOT NULL DEFAULT 0,
                 acknowledged_segments INTEGER NOT NULL DEFAULT 0,
                 acknowledged_bytes INTEGER NOT NULL DEFAULT 0,
                 applied_segments INTEGER NOT NULL DEFAULT 0,
                 applied_bytes INTEGER NOT NULL DEFAULT 0,
                 source_delivery_attempts INTEGER NOT NULL DEFAULT 0,
                 source_delivery_successes INTEGER NOT NULL DEFAULT 0,
                 source_transport_errors INTEGER NOT NULL DEFAULT 0,
                 source_response_errors INTEGER NOT NULL DEFAULT 0,
                 source_response_changed INTEGER NOT NULL DEFAULT 0,
                 source_explicit_unavailable INTEGER NOT NULL DEFAULT 0,
                 source_storage_errors INTEGER NOT NULL DEFAULT 0,
                 last_attempt_at INTEGER,
                 last_progress_at INTEGER,
                 counter_epoch INTEGER NOT NULL DEFAULT 1
             );
             INSERT INTO repository_delivery_metrics (singleton)
             VALUES (1)
             ON CONFLICT(singleton) DO NOTHING;",
        )
        .map_err(sqlite_error)?;
    let mut statement = transaction
        .prepare("PRAGMA table_info(repository_delivery_metrics)")
        .map_err(sqlite_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite_error)?
        .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()
        .map_err(sqlite_error)?;
    drop(statement);
    for (name, definition) in [
        ("generated_segments", "INTEGER NOT NULL DEFAULT 0"),
        ("generated_bytes", "INTEGER NOT NULL DEFAULT 0"),
        ("acknowledged_segments", "INTEGER NOT NULL DEFAULT 0"),
        ("acknowledged_bytes", "INTEGER NOT NULL DEFAULT 0"),
        ("applied_segments", "INTEGER NOT NULL DEFAULT 0"),
        ("applied_bytes", "INTEGER NOT NULL DEFAULT 0"),
        ("source_delivery_attempts", "INTEGER NOT NULL DEFAULT 0"),
        ("source_delivery_successes", "INTEGER NOT NULL DEFAULT 0"),
        ("source_transport_errors", "INTEGER NOT NULL DEFAULT 0"),
        ("source_response_errors", "INTEGER NOT NULL DEFAULT 0"),
        ("source_response_changed", "INTEGER NOT NULL DEFAULT 0"),
        ("source_explicit_unavailable", "INTEGER NOT NULL DEFAULT 0"),
        ("source_storage_errors", "INTEGER NOT NULL DEFAULT 0"),
        ("last_attempt_at", "INTEGER"),
        ("last_progress_at", "INTEGER"),
        ("counter_epoch", "INTEGER NOT NULL DEFAULT 1"),
    ] {
        if !columns.contains(name) {
            transaction
                .execute(
                    &format!(
                        "ALTER TABLE repository_delivery_metrics ADD COLUMN {name} {definition}"
                    ),
                    [],
                )
                .map_err(sqlite_error)?;
        }
    }
    transaction.commit().map_err(sqlite_error)
}

fn add_counter(transaction: &rusqlite::Transaction<'_>, column: &str, value: u64) -> Result<()> {
    if value == 0 {
        return Ok(());
    }
    let value = i64::try_from(value).map_err(|_| {
        HistoryStorageError(format!("delivery metric {column} exceeds SQLite integer"))
    })?;
    let updated = transaction
        .execute(
            &format!(
                "UPDATE repository_delivery_metrics
                 SET {column} = {column} + ?1
                 WHERE singleton = 1
                   AND {column} <= 9223372036854775807 - ?1"
            ),
            [value],
        )
        .map_err(sqlite_error)?;
    if updated != 1 {
        return Err(HistoryStorageError(format!(
            "delivery metric {column} exceeds SQLite integer"
        )));
    }
    Ok(())
}

pub(super) fn increment_generated(
    transaction: &rusqlite::Transaction<'_>,
    segments: u64,
    bytes: u64,
    progress_at: Option<u64>,
) -> Result<()> {
    add_counter(transaction, "generated_segments", segments)?;
    add_counter(transaction, "generated_bytes", bytes)?;
    set_progress_at(transaction, progress_at)
}

pub(super) fn increment_acknowledged(
    transaction: &rusqlite::Transaction<'_>,
    segments: u64,
    bytes: u64,
    progress_at: Option<u64>,
) -> Result<()> {
    add_counter(transaction, "acknowledged_segments", segments)?;
    add_counter(transaction, "acknowledged_bytes", bytes)?;
    set_progress_at(transaction, progress_at)
}

pub(super) fn increment_applied(
    transaction: &rusqlite::Transaction<'_>,
    segments: u64,
    bytes: u64,
    progress_at: Option<u64>,
) -> Result<()> {
    add_counter(transaction, "applied_segments", segments)?;
    add_counter(transaction, "applied_bytes", bytes)?;
    set_progress_at(transaction, progress_at)
}

pub(super) fn record_source_attempt(
    transaction: &rusqlite::Transaction<'_>,
    attempt: SourceDeliveryAttempt,
) -> Result<()> {
    add_counter(transaction, "source_delivery_attempts", attempt.attempts)?;
    add_counter(transaction, "source_delivery_successes", attempt.successes)?;
    let column = match attempt.outcome {
        "success" => None,
        "transport_error" => Some("source_transport_errors"),
        "response_error" => Some("source_response_errors"),
        "response_changed" => Some("source_response_changed"),
        "explicit_unavailable" => Some("source_explicit_unavailable"),
        "storage_error" => Some("source_storage_errors"),
        _ => {
            return Err(HistoryStorageError(format!(
                "unknown source delivery outcome: {}",
                attempt.outcome
            )));
        }
    };
    if let Some(column) = column {
        add_counter(transaction, column, 1)?;
    }
    transaction
        .execute(
            "UPDATE repository_delivery_metrics
             SET last_attempt_at = ?1
             WHERE singleton = 1",
            [durable_i64(
                attempt.now_unix_seconds,
                "delivery attempt time",
            )?],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn set_progress_at(
    transaction: &rusqlite::Transaction<'_>,
    progress_at: Option<u64>,
) -> Result<()> {
    if let Some(progress_at) = progress_at {
        transaction
            .execute(
                "UPDATE repository_delivery_metrics
                 SET last_progress_at = MAX(COALESCE(last_progress_at, 0), ?1)
                 WHERE singleton = 1",
                [durable_i64(progress_at, "delivery progress time")?],
            )
            .map_err(sqlite_error)?;
    }
    Ok(())
}

pub(super) fn read(connection: &Connection) -> Result<RepositoryDeliveryMetrics> {
    connection
        .query_row(
            "SELECT generated_segments, generated_bytes,
                    acknowledged_segments, acknowledged_bytes,
                    applied_segments, applied_bytes,
                    source_delivery_attempts, source_delivery_successes,
                    source_transport_errors, source_response_errors,
                    source_response_changed,
                    source_explicit_unavailable, source_storage_errors,
                    last_attempt_at, last_progress_at,
                    counter_epoch
             FROM repository_delivery_metrics WHERE singleton = 1",
            [],
            |row| {
                Ok(RepositoryDeliveryMetrics {
                    generated_segments: checked_u64(row.get::<_, i64>(0)?, 0)?,
                    generated_bytes: checked_u64(row.get::<_, i64>(1)?, 1)?,
                    acknowledged_segments: checked_u64(row.get::<_, i64>(2)?, 2)?,
                    acknowledged_bytes: checked_u64(row.get::<_, i64>(3)?, 3)?,
                    applied_segments: checked_u64(row.get::<_, i64>(4)?, 4)?,
                    applied_bytes: checked_u64(row.get::<_, i64>(5)?, 5)?,
                    source_delivery_attempts: checked_u64(row.get::<_, i64>(6)?, 6)?,
                    source_delivery_successes: checked_u64(row.get::<_, i64>(7)?, 7)?,
                    source_transport_errors: checked_u64(row.get::<_, i64>(8)?, 8)?,
                    source_response_errors: checked_u64(row.get::<_, i64>(9)?, 9)?,
                    source_response_changed: checked_u64(row.get::<_, i64>(10)?, 10)?,
                    source_explicit_unavailable: checked_u64(row.get::<_, i64>(11)?, 11)?,
                    source_storage_errors: checked_u64(row.get::<_, i64>(12)?, 12)?,
                    last_attempt_at: row
                        .get::<_, Option<i64>>(13)?
                        .map(|value| checked_u64(value, 13))
                        .transpose()?,
                    last_progress_at: row
                        .get::<_, Option<i64>>(14)?
                        .map(|value| checked_u64(value, 14))
                        .transpose()?,
                    counter_epoch: checked_u64(row.get::<_, i64>(15)?, 15)?,
                    rate_window_seconds: DELIVERY_RATE_WINDOW_SECONDS,
                })
            },
        )
        .map_err(sqlite_error)
}

fn checked_u64(value: i64, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

impl HistoryStorage {
    pub(crate) fn repository_delivery_metrics(&self) -> Result<RepositoryDeliveryMetrics> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(RepositoryDeliveryMetrics::default());
        };
        read(connection)
    }

    pub(crate) fn record_source_delivery_attempt(
        &self,
        attempt: SourceDeliveryAttempt,
    ) -> Result<()> {
        if attempt.attempts == 0 {
            return Ok(());
        }
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(());
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        record_source_attempt(&transaction, attempt)?;
        transaction.commit().map_err(sqlite_error)
    }
}
