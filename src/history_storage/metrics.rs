use serde::{Deserialize, Serialize};

use super::*;

#[path = "metrics_scopes.rs"]
mod scopes;
pub(super) use scopes::{
    increment_acknowledged_scope, increment_applied_scope, increment_generated_scope,
};

pub(crate) const DELIVERY_RATE_WINDOW_SECONDS: u64 = 5 * 60;

fn default_delivery_rate_window_seconds() -> u64 {
    DELIVERY_RATE_WINDOW_SECONDS
}

#[derive(Debug, Clone)]
pub(crate) struct SourceDeliveryAttempt {
    pub(crate) now_unix_seconds: u64,
    pub(crate) attempts: u64,
    pub(crate) successes: u64,
    pub(crate) outcome: &'static str,
    pub(crate) target_repository_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_completed_window: Option<RepositoryDeliveryWindow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) delivery_scopes: Vec<RepositoryDeliveryScopeMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepositoryDeliveryScopeMetrics {
    pub(crate) source_node_id: String,
    pub(crate) stream: String,
    /// Empty means source generation has not yet been assigned to a collector. A non-empty
    /// value is the registered target repository identity used for ACK/apply accounting.
    pub(crate) target_repository_id: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_completed_window: Option<RepositoryDeliveryWindow>,
}

/// A bounded observation derived from two durable metric snapshots. Keeping this calculation
/// separate from the persisted counters makes the five-minute denominator explicit and prevents
/// queue-depth deltas from being mistaken for processing capacity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepositoryDeliveryWindow {
    pub(crate) started_at: u64,
    pub(crate) ended_at: u64,
    pub(crate) elapsed_seconds: u64,
    pub(crate) generated_segments: u64,
    pub(crate) generated_bytes: u64,
    pub(crate) acknowledged_segments: u64,
    pub(crate) acknowledged_bytes: u64,
    pub(crate) applied_segments: u64,
    pub(crate) applied_bytes: u64,
    pub(crate) generated_segments_per_second_milli: u64,
    pub(crate) generated_bytes_per_second_milli: u64,
    pub(crate) acknowledged_segments_per_second_milli: u64,
    pub(crate) acknowledged_bytes_per_second_milli: u64,
    pub(crate) applied_segments_per_second_milli: u64,
    pub(crate) applied_bytes_per_second_milli: u64,
    pub(crate) source_ack_meets_ten_x_segments: bool,
    pub(crate) source_ack_meets_ten_x_bytes: bool,
    pub(crate) source_ack_meets_ten_x: bool,
    pub(crate) repository_apply_meets_ten_x_segments: bool,
    pub(crate) repository_apply_meets_ten_x_bytes: bool,
    pub(crate) repository_apply_meets_ten_x: bool,
    pub(crate) meets_ten_x: bool,
}

impl RepositoryDeliveryWindow {
    #[allow(clippy::too_many_arguments)]
    fn from_deltas(
        started_at: u64,
        ended_at: u64,
        elapsed_seconds: u64,
        generated_segments: u64,
        generated_bytes: u64,
        acknowledged_segments: u64,
        acknowledged_bytes: u64,
        applied_segments: u64,
        applied_bytes: u64,
    ) -> Self {
        let elapsed_seconds = elapsed_seconds.max(1);
        let generated_segments_per_second_milli =
            generated_segments.saturating_mul(1_000) / elapsed_seconds;
        let generated_bytes_per_second_milli =
            generated_bytes.saturating_mul(1_000) / elapsed_seconds;
        let acknowledged_segments_per_second_milli =
            acknowledged_segments.saturating_mul(1_000) / elapsed_seconds;
        let acknowledged_bytes_per_second_milli =
            acknowledged_bytes.saturating_mul(1_000) / elapsed_seconds;
        let applied_segments_per_second_milli =
            applied_segments.saturating_mul(1_000) / elapsed_seconds;
        let applied_bytes_per_second_milli = applied_bytes.saturating_mul(1_000) / elapsed_seconds;
        let source_ack_meets_ten_x_segments = generated_segments > 0
            && acknowledged_segments >= generated_segments.saturating_mul(10);
        let source_ack_meets_ten_x_bytes =
            generated_bytes > 0 && acknowledged_bytes >= generated_bytes.saturating_mul(10);
        let repository_apply_meets_ten_x_segments =
            generated_segments > 0 && applied_segments >= generated_segments.saturating_mul(10);
        let repository_apply_meets_ten_x_bytes =
            generated_bytes > 0 && applied_bytes >= generated_bytes.saturating_mul(10);
        let source_ack_meets_ten_x = elapsed_seconds >= DELIVERY_RATE_WINDOW_SECONDS
            && source_ack_meets_ten_x_segments
            && source_ack_meets_ten_x_bytes;
        let repository_apply_meets_ten_x = elapsed_seconds >= DELIVERY_RATE_WINDOW_SECONDS
            && repository_apply_meets_ten_x_segments
            && repository_apply_meets_ten_x_bytes;
        Self {
            started_at,
            ended_at,
            elapsed_seconds,
            generated_segments,
            generated_bytes,
            acknowledged_segments,
            acknowledged_bytes,
            applied_segments,
            applied_bytes,
            generated_segments_per_second_milli,
            generated_bytes_per_second_milli,
            acknowledged_segments_per_second_milli,
            acknowledged_bytes_per_second_milli,
            applied_segments_per_second_milli,
            applied_bytes_per_second_milli,
            source_ack_meets_ten_x_segments,
            source_ack_meets_ten_x_bytes,
            source_ack_meets_ten_x,
            repository_apply_meets_ten_x_segments,
            repository_apply_meets_ten_x_bytes,
            repository_apply_meets_ten_x,
            meets_ten_x: source_ack_meets_ten_x && repository_apply_meets_ten_x,
        }
    }

    #[cfg(test)]
    pub(crate) fn source_ack_meets_ten_x(&self) -> bool {
        self.source_ack_meets_ten_x
    }

    #[cfg(test)]
    pub(crate) fn repository_apply_meets_ten_x(&self) -> bool {
        self.repository_apply_meets_ten_x
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
            last_completed_window: None,
            delivery_scopes: Vec::new(),
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
        Some(RepositoryDeliveryWindow::from_deltas(
            0,
            elapsed_seconds,
            elapsed_seconds,
            self.generated_segments - start.generated_segments,
            self.generated_bytes - start.generated_bytes,
            self.acknowledged_segments - start.acknowledged_segments,
            self.acknowledged_bytes - start.acknowledged_bytes,
            self.applied_segments - start.applied_segments,
            self.applied_bytes - start.applied_bytes,
        ))
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
                 counter_epoch INTEGER NOT NULL DEFAULT 1,
                 window_started_at INTEGER,
                 window_generated_segments INTEGER,
                 window_generated_bytes INTEGER,
                 window_acknowledged_segments INTEGER,
                 window_acknowledged_bytes INTEGER,
                 window_applied_segments INTEGER,
                 window_applied_bytes INTEGER,
                 completed_window_started_at INTEGER,
                 completed_window_ended_at INTEGER,
                 completed_generated_segments INTEGER,
                 completed_generated_bytes INTEGER,
                 completed_acknowledged_segments INTEGER,
                 completed_acknowledged_bytes INTEGER,
                 completed_applied_segments INTEGER,
                 completed_applied_bytes INTEGER
             );
             INSERT INTO repository_delivery_metrics (singleton)
             VALUES (1)
             ON CONFLICT(singleton) DO NOTHING;",
        )
        .map_err(sqlite_error)?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS repository_delivery_metric_scopes (
                 source_node_id TEXT NOT NULL,
                 stream TEXT NOT NULL,
                 target_repository_id TEXT NOT NULL DEFAULT '',
                 generated_segments INTEGER NOT NULL DEFAULT 0,
                 generated_bytes INTEGER NOT NULL DEFAULT 0,
                 acknowledged_segments INTEGER NOT NULL DEFAULT 0,
                 acknowledged_bytes INTEGER NOT NULL DEFAULT 0,
                 applied_segments INTEGER NOT NULL DEFAULT 0,
                 applied_bytes INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (source_node_id, stream, target_repository_id)
             );
             CREATE TABLE IF NOT EXISTS repository_delivery_metric_scope_windows (
                 source_node_id TEXT NOT NULL,
                 stream TEXT NOT NULL,
                 target_repository_id TEXT NOT NULL DEFAULT '',
                 window_started_at INTEGER,
                 window_generated_segments INTEGER,
                 window_generated_bytes INTEGER,
                 window_acknowledged_segments INTEGER,
                 window_acknowledged_bytes INTEGER,
                 window_applied_segments INTEGER,
                 window_applied_bytes INTEGER,
                 completed_window_started_at INTEGER,
                 completed_window_ended_at INTEGER,
                 completed_generated_segments INTEGER,
                 completed_generated_bytes INTEGER,
                 completed_acknowledged_segments INTEGER,
                 completed_acknowledged_bytes INTEGER,
                 completed_applied_segments INTEGER,
                 completed_applied_bytes INTEGER,
                 PRIMARY KEY (source_node_id, stream, target_repository_id)
             );",
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
        ("window_started_at", "INTEGER"),
        ("window_generated_segments", "INTEGER"),
        ("window_generated_bytes", "INTEGER"),
        ("window_acknowledged_segments", "INTEGER"),
        ("window_acknowledged_bytes", "INTEGER"),
        ("window_applied_segments", "INTEGER"),
        ("window_applied_bytes", "INTEGER"),
        ("completed_window_started_at", "INTEGER"),
        ("completed_window_ended_at", "INTEGER"),
        ("completed_generated_segments", "INTEGER"),
        ("completed_generated_bytes", "INTEGER"),
        ("completed_acknowledged_segments", "INTEGER"),
        ("completed_acknowledged_bytes", "INTEGER"),
        ("completed_applied_segments", "INTEGER"),
        ("completed_applied_bytes", "INTEGER"),
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

fn add_counter_pair(
    transaction: &rusqlite::Transaction<'_>,
    first_column: &str,
    first_value: u64,
    second_column: &str,
    second_value: u64,
) -> Result<()> {
    if first_value == 0 && second_value == 0 {
        return Ok(());
    }
    let first_value = i64::try_from(first_value).map_err(|_| {
        HistoryStorageError(format!(
            "delivery metric {first_column} exceeds SQLite integer"
        ))
    })?;
    let second_value = i64::try_from(second_value).map_err(|_| {
        HistoryStorageError(format!(
            "delivery metric {second_column} exceeds SQLite integer"
        ))
    })?;
    let updated = transaction
        .execute(
            &format!(
                "UPDATE repository_delivery_metrics
                 SET {first_column} = {first_column} + ?1,
                     {second_column} = {second_column} + ?2
                 WHERE singleton = 1
                   AND {first_column} <= 9223372036854775807 - ?1
                   AND {second_column} <= 9223372036854775807 - ?2"
            ),
            rusqlite::params![first_value, second_value],
        )
        .map_err(sqlite_error)?;
    if updated != 1 {
        return Err(HistoryStorageError(format!(
            "delivery metric {first_column}/{second_column} exceeds SQLite integer"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct DeliveryWindowState {
    generated_segments: u64,
    generated_bytes: u64,
    acknowledged_segments: u64,
    acknowledged_bytes: u64,
    applied_segments: u64,
    applied_bytes: u64,
    generated_segments_at_start: u64,
    generated_bytes_at_start: u64,
    acknowledged_segments_at_start: u64,
    acknowledged_bytes_at_start: u64,
    applied_segments_at_start: u64,
    applied_bytes_at_start: u64,
}

fn read_counter_baseline(transaction: &rusqlite::Transaction<'_>) -> Result<DeliveryWindowState> {
    transaction
        .query_row(
            "SELECT generated_segments, generated_bytes,
                    acknowledged_segments, acknowledged_bytes,
                    applied_segments, applied_bytes,
                    window_started_at,
                    window_generated_segments, window_generated_bytes,
                    window_acknowledged_segments, window_acknowledged_bytes,
                    window_applied_segments, window_applied_bytes
             FROM repository_delivery_metrics WHERE singleton = 1",
            [],
            |row| {
                Ok(DeliveryWindowState {
                    generated_segments: checked_u64(row.get::<_, i64>(0)?, 0)?,
                    generated_bytes: checked_u64(row.get::<_, i64>(1)?, 1)?,
                    acknowledged_segments: checked_u64(row.get::<_, i64>(2)?, 2)?,
                    acknowledged_bytes: checked_u64(row.get::<_, i64>(3)?, 3)?,
                    applied_segments: checked_u64(row.get::<_, i64>(4)?, 4)?,
                    applied_bytes: checked_u64(row.get::<_, i64>(5)?, 5)?,
                    generated_segments_at_start: row
                        .get::<_, Option<i64>>(7)?
                        .map(|value| checked_u64(value, 7))
                        .transpose()?
                        .unwrap_or(0),
                    generated_bytes_at_start: row
                        .get::<_, Option<i64>>(8)?
                        .map(|value| checked_u64(value, 8))
                        .transpose()?
                        .unwrap_or(0),
                    acknowledged_segments_at_start: row
                        .get::<_, Option<i64>>(9)?
                        .map(|value| checked_u64(value, 9))
                        .transpose()?
                        .unwrap_or(0),
                    acknowledged_bytes_at_start: row
                        .get::<_, Option<i64>>(10)?
                        .map(|value| checked_u64(value, 10))
                        .transpose()?
                        .unwrap_or(0),
                    applied_segments_at_start: row
                        .get::<_, Option<i64>>(11)?
                        .map(|value| checked_u64(value, 11))
                        .transpose()?
                        .unwrap_or(0),
                    applied_bytes_at_start: row
                        .get::<_, Option<i64>>(12)?
                        .map(|value| checked_u64(value, 12))
                        .transpose()?
                        .unwrap_or(0),
                })
            },
        )
        .map_err(sqlite_error)
}

#[allow(clippy::too_many_arguments)]
fn set_window_baseline(
    transaction: &rusqlite::Transaction<'_>,
    started_at: u64,
    generated_segments: u64,
    generated_bytes: u64,
    acknowledged_segments: u64,
    acknowledged_bytes: u64,
    applied_segments: u64,
    applied_bytes: u64,
) -> Result<()> {
    transaction
        .execute(
            "UPDATE repository_delivery_metrics
             SET window_started_at = ?1,
                 window_generated_segments = ?2,
                 window_generated_bytes = ?3,
                 window_acknowledged_segments = ?4,
                 window_acknowledged_bytes = ?5,
                 window_applied_segments = ?6,
                 window_applied_bytes = ?7
             WHERE singleton = 1",
            rusqlite::params![
                durable_i64(started_at, "delivery window start")?,
                durable_i64(generated_segments, "window generated segments")?,
                durable_i64(generated_bytes, "window generated bytes")?,
                durable_i64(acknowledged_segments, "window acknowledged segments")?,
                durable_i64(acknowledged_bytes, "window acknowledged bytes")?,
                durable_i64(applied_segments, "window applied segments")?,
                durable_i64(applied_bytes, "window applied bytes")?,
            ],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn rotate_window_if_elapsed(
    transaction: &rusqlite::Transaction<'_>,
    progress_at: Option<u64>,
) -> Result<()> {
    let Some(progress_at) = progress_at else {
        return Ok(());
    };
    let window_started_at = transaction
        .query_row(
            "SELECT window_started_at
             FROM repository_delivery_metrics WHERE singleton = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(sqlite_error)?
        .map(|value| checked_u64(value, 0))
        .transpose()
        .map_err(sqlite_error)?;
    let Some(window_started_at) = window_started_at else {
        let state = read_counter_baseline(transaction)?;
        return set_window_baseline(
            transaction,
            progress_at,
            state.generated_segments,
            state.generated_bytes,
            state.acknowledged_segments,
            state.acknowledged_bytes,
            state.applied_segments,
            state.applied_bytes,
        );
    };
    if progress_at < window_started_at.saturating_add(DELIVERY_RATE_WINDOW_SECONDS) {
        return Ok(());
    }
    let state = read_counter_baseline(transaction)?;
    let window = RepositoryDeliveryWindow::from_deltas(
        window_started_at,
        window_started_at.saturating_add(DELIVERY_RATE_WINDOW_SECONDS),
        DELIVERY_RATE_WINDOW_SECONDS,
        state
            .generated_segments
            .saturating_sub(state.generated_segments_at_start),
        state
            .generated_bytes
            .saturating_sub(state.generated_bytes_at_start),
        state
            .acknowledged_segments
            .saturating_sub(state.acknowledged_segments_at_start),
        state
            .acknowledged_bytes
            .saturating_sub(state.acknowledged_bytes_at_start),
        state
            .applied_segments
            .saturating_sub(state.applied_segments_at_start),
        state
            .applied_bytes
            .saturating_sub(state.applied_bytes_at_start),
    );
    transaction
        .execute(
            "UPDATE repository_delivery_metrics
             SET completed_window_started_at = ?1,
                 completed_window_ended_at = ?2,
                 completed_generated_segments = ?3,
                 completed_generated_bytes = ?4,
                 completed_acknowledged_segments = ?5,
                 completed_acknowledged_bytes = ?6,
                 completed_applied_segments = ?7,
                 completed_applied_bytes = ?8
             WHERE singleton = 1",
            rusqlite::params![
                durable_i64(window.started_at, "completed window start")?,
                durable_i64(window.ended_at, "completed window end")?,
                durable_i64(window.generated_segments, "completed generated segments")?,
                durable_i64(window.generated_bytes, "completed generated bytes")?,
                durable_i64(
                    window.acknowledged_segments,
                    "completed acknowledged segments"
                )?,
                durable_i64(window.acknowledged_bytes, "completed acknowledged bytes")?,
                durable_i64(window.applied_segments, "completed applied segments")?,
                durable_i64(window.applied_bytes, "completed applied bytes")?,
            ],
        )
        .map_err(sqlite_error)?;
    set_window_baseline(
        transaction,
        progress_at,
        state.generated_segments,
        state.generated_bytes,
        state.acknowledged_segments,
        state.acknowledged_bytes,
        state.applied_segments,
        state.applied_bytes,
    )
}

pub(super) fn increment_generated(
    transaction: &rusqlite::Transaction<'_>,
    segments: u64,
    bytes: u64,
    progress_at: Option<u64>,
) -> Result<()> {
    rotate_window_if_elapsed(transaction, progress_at)?;
    add_counter_pair(
        transaction,
        "generated_segments",
        segments,
        "generated_bytes",
        bytes,
    )?;
    set_progress_at(transaction, progress_at)
}

pub(super) fn increment_acknowledged(
    transaction: &rusqlite::Transaction<'_>,
    segments: u64,
    bytes: u64,
    progress_at: Option<u64>,
) -> Result<()> {
    rotate_window_if_elapsed(transaction, progress_at)?;
    add_counter_pair(
        transaction,
        "acknowledged_segments",
        segments,
        "acknowledged_bytes",
        bytes,
    )?;
    set_progress_at(transaction, progress_at)
}

pub(super) fn increment_applied(
    transaction: &rusqlite::Transaction<'_>,
    segments: u64,
    bytes: u64,
    progress_at: Option<u64>,
) -> Result<()> {
    rotate_window_if_elapsed(transaction, progress_at)?;
    add_counter_pair(
        transaction,
        "applied_segments",
        segments,
        "applied_bytes",
        bytes,
    )?;
    set_progress_at(transaction, progress_at)
}

pub(super) fn record_source_attempt(
    transaction: &rusqlite::Transaction<'_>,
    attempt: SourceDeliveryAttempt,
) -> Result<()> {
    rotate_window_if_elapsed(transaction, Some(attempt.now_unix_seconds))?;
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

pub(super) fn read_at(
    connection: &Connection,
    now_unix_seconds: Option<u64>,
) -> Result<RepositoryDeliveryMetrics> {
    let mut metrics = connection
        .query_row(
            "SELECT generated_segments, generated_bytes,
                    acknowledged_segments, acknowledged_bytes,
                    applied_segments, applied_bytes,
                    source_delivery_attempts, source_delivery_successes,
                    source_transport_errors, source_response_errors,
                    source_response_changed,
                    source_explicit_unavailable, source_storage_errors,
                    last_attempt_at, last_progress_at,
                    counter_epoch,
                    window_started_at, window_generated_segments,
                    window_generated_bytes, window_acknowledged_segments,
                    window_acknowledged_bytes, window_applied_segments,
                    window_applied_bytes,
                    completed_window_started_at, completed_window_ended_at,
                    completed_generated_segments, completed_generated_bytes,
                    completed_acknowledged_segments, completed_acknowledged_bytes,
                    completed_applied_segments, completed_applied_bytes
             FROM repository_delivery_metrics WHERE singleton = 1",
            [],
            |row| {
                let generated_segments = checked_u64(row.get::<_, i64>(0)?, 0)?;
                let generated_bytes = checked_u64(row.get::<_, i64>(1)?, 1)?;
                let acknowledged_segments = checked_u64(row.get::<_, i64>(2)?, 2)?;
                let acknowledged_bytes = checked_u64(row.get::<_, i64>(3)?, 3)?;
                let applied_segments = checked_u64(row.get::<_, i64>(4)?, 4)?;
                let applied_bytes = checked_u64(row.get::<_, i64>(5)?, 5)?;
                let window_started_at = row
                    .get::<_, Option<i64>>(16)?
                    .map(|value| checked_u64(value, 16))
                    .transpose()?;
                let window =
                    |start_column: usize,
                     end_column: usize,
                     elapsed_seconds: u64|
                     -> rusqlite::Result<Option<RepositoryDeliveryWindow>> {
                        let Some(started_at) = row
                            .get::<_, Option<i64>>(start_column)?
                            .map(|value| checked_u64(value, start_column))
                            .transpose()?
                        else {
                            return Ok(None);
                        };
                        let Some(ended_at) = row
                            .get::<_, Option<i64>>(end_column)?
                            .map(|value| checked_u64(value, end_column))
                            .transpose()?
                        else {
                            return Ok(None);
                        };
                        let values = [
                            row.get::<_, Option<i64>>(25)?,
                            row.get::<_, Option<i64>>(26)?,
                            row.get::<_, Option<i64>>(27)?,
                            row.get::<_, Option<i64>>(28)?,
                            row.get::<_, Option<i64>>(29)?,
                            row.get::<_, Option<i64>>(30)?,
                        ];
                        let values = values
                            .into_iter()
                            .enumerate()
                            .map(|(index, value)| {
                                value
                                    .map(|value| checked_u64(value, 25 + index))
                                    .transpose()
                                    .map(|value| value.unwrap_or(0))
                            })
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        Ok(Some(RepositoryDeliveryWindow::from_deltas(
                            started_at,
                            ended_at,
                            elapsed_seconds,
                            values[0],
                            values[1],
                            values[2],
                            values[3],
                            values[4],
                            values[5],
                        )))
                    };
                let last_completed_window = window(23, 24, DELIVERY_RATE_WINDOW_SECONDS)?;
                let last_completed_window = match last_completed_window {
                    Some(window) => Some(window),
                    None => match (now_unix_seconds, window_started_at) {
                        (Some(now), Some(started_at))
                            if now >= started_at.saturating_add(DELIVERY_RATE_WINDOW_SECONDS) =>
                        {
                            let baselines = [
                                row.get::<_, Option<i64>>(17)?,
                                row.get::<_, Option<i64>>(18)?,
                                row.get::<_, Option<i64>>(19)?,
                                row.get::<_, Option<i64>>(20)?,
                                row.get::<_, Option<i64>>(21)?,
                                row.get::<_, Option<i64>>(22)?,
                            ];
                            let baselines = baselines
                                .into_iter()
                                .enumerate()
                                .map(|(index, value)| {
                                    value
                                        .map(|value| checked_u64(value, 17 + index))
                                        .transpose()
                                        .map(|value| value.unwrap_or(0))
                                })
                                .collect::<rusqlite::Result<Vec<_>>>()?;
                            Some(RepositoryDeliveryWindow::from_deltas(
                                started_at,
                                started_at.saturating_add(DELIVERY_RATE_WINDOW_SECONDS),
                                DELIVERY_RATE_WINDOW_SECONDS,
                                generated_segments.saturating_sub(baselines[0]),
                                generated_bytes.saturating_sub(baselines[1]),
                                acknowledged_segments.saturating_sub(baselines[2]),
                                acknowledged_bytes.saturating_sub(baselines[3]),
                                applied_segments.saturating_sub(baselines[4]),
                                applied_bytes.saturating_sub(baselines[5]),
                            ))
                        }
                        _ => None,
                    },
                };
                Ok(RepositoryDeliveryMetrics {
                    generated_segments,
                    generated_bytes,
                    acknowledged_segments,
                    acknowledged_bytes,
                    applied_segments,
                    applied_bytes,
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
                    last_completed_window,
                    delivery_scopes: Vec::new(),
                })
            },
        )
        .map_err(sqlite_error)?;
    metrics.delivery_scopes = scopes::read_scopes(connection, now_unix_seconds)?;
    Ok(metrics)
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
    #[cfg(test)]
    pub(crate) fn repository_delivery_metrics(&self) -> Result<RepositoryDeliveryMetrics> {
        self.repository_delivery_metrics_at(None)
    }

    pub(crate) fn repository_delivery_metrics_at(
        &self,
        now_unix_seconds: Option<u64>,
    ) -> Result<RepositoryDeliveryMetrics> {
        let mut backend = self.lock_backend();
        let Some(connection) = sqlite_connection(&mut backend)? else {
            return Ok(RepositoryDeliveryMetrics::default());
        };
        read_at(connection, now_unix_seconds)
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
