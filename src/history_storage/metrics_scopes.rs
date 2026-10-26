use super::{
    DELIVERY_RATE_WINDOW_SECONDS, RepositoryDeliveryScopeMetrics, RepositoryDeliveryWindow,
};
use crate::state::history_storage::{HistoryStorageError, Result, durable_i64, sqlite_error};
use rusqlite::{Connection, OptionalExtension};
#[allow(clippy::too_many_arguments)]
fn add_scope_counters(
    transaction: &rusqlite::Transaction<'_>,
    source_node_id: &str,
    stream: &str,
    target_repository_id: &str,
    segments_column: &str,
    segments: u64,
    bytes_column: &str,
    bytes: u64,
) -> Result<()> {
    if segments == 0 && bytes == 0 {
        return Ok(());
    }
    let segments = i64::try_from(segments).map_err(|_| {
        HistoryStorageError(format!(
            "delivery scope metric {segments_column} exceeds SQLite integer"
        ))
    })?;
    let bytes = i64::try_from(bytes).map_err(|_| {
        HistoryStorageError(format!(
            "delivery scope metric {bytes_column} exceeds SQLite integer"
        ))
    })?;
    let updated = transaction
        .execute(
            &format!(
                "UPDATE repository_delivery_metric_scopes
                 SET {segments_column} = {segments_column} + ?1,
                     {bytes_column} = {bytes_column} + ?2
                 WHERE source_node_id = ?3
                   AND stream = ?4
                   AND target_repository_id = ?5
                   AND {segments_column} <= 9223372036854775807 - ?1
                   AND {bytes_column} <= 9223372036854775807 - ?2"
            ),
            rusqlite::params![
                segments,
                bytes,
                source_node_id,
                stream,
                target_repository_id
            ],
        )
        .map_err(sqlite_error)?;
    if updated != 1 {
        return Err(HistoryStorageError(format!(
            "delivery scope metric {segments_column}/{bytes_column} exceeds SQLite integer"
        )));
    }
    Ok(())
}

pub(super) fn ensure_scope_row(
    transaction: &rusqlite::Transaction<'_>,
    source_node_id: &str,
    stream: &str,
    target_repository_id: &str,
    inherit_window: bool,
) -> Result<()> {
    if target_repository_id.is_empty() {
        transaction
            .execute(
                "INSERT INTO repository_delivery_metric_scopes
                     (source_node_id, stream, target_repository_id)
                 VALUES (?1, ?2, '')
                 ON CONFLICT(source_node_id, stream, target_repository_id) DO NOTHING",
                rusqlite::params![source_node_id, stream],
            )
            .map_err(sqlite_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO repository_delivery_metric_scopes
                     (source_node_id, stream, target_repository_id,
                      generated_segments, generated_bytes)
                 SELECT ?1, ?2, ?3,
                        COALESCE((SELECT generated_segments
                                  FROM repository_delivery_metric_scopes
                                  WHERE source_node_id = ?1 AND stream = ?2
                                    AND target_repository_id = ''), 0),
                        COALESCE((SELECT generated_bytes
                                  FROM repository_delivery_metric_scopes
                                  WHERE source_node_id = ?1 AND stream = ?2
                                    AND target_repository_id = ''), 0)
                 ON CONFLICT(source_node_id, stream, target_repository_id) DO NOTHING",
                rusqlite::params![source_node_id, stream, target_repository_id],
            )
            .map_err(sqlite_error)?;
    }
    if inherit_window {
        // A target known before generation starts shares the aggregate five-minute baseline.
        transaction
            .execute(
                "INSERT INTO repository_delivery_metric_scope_windows
                     (source_node_id, stream, target_repository_id,
                      window_started_at, window_generated_segments, window_generated_bytes,
                      window_acknowledged_segments, window_acknowledged_bytes,
                      window_applied_segments, window_applied_bytes,
                      completed_window_started_at, completed_window_ended_at,
                      completed_generated_segments, completed_generated_bytes,
                      completed_acknowledged_segments, completed_acknowledged_bytes,
                      completed_applied_segments, completed_applied_bytes,
                      completed_peak_generated_segments, completed_peak_generated_bytes)
                 SELECT ?1, ?2, ?3,
                        window_started_at, window_generated_segments, window_generated_bytes,
                        window_acknowledged_segments, window_acknowledged_bytes,
                        window_applied_segments, window_applied_bytes,
                        completed_window_started_at, completed_window_ended_at,
                        completed_generated_segments, completed_generated_bytes,
                        completed_acknowledged_segments, completed_acknowledged_bytes,
                        completed_applied_segments, completed_applied_bytes,
                        completed_peak_generated_segments, completed_peak_generated_bytes
                 FROM repository_delivery_metric_scope_windows
                 WHERE source_node_id = ?1 AND stream = ?2 AND target_repository_id = ''
                 ON CONFLICT(source_node_id, stream, target_repository_id) DO UPDATE SET
                     window_started_at = excluded.window_started_at,
                     window_generated_segments = excluded.window_generated_segments,
                     window_generated_bytes = excluded.window_generated_bytes,
                     window_acknowledged_segments = excluded.window_acknowledged_segments,
                     window_acknowledged_bytes = excluded.window_acknowledged_bytes,
                     window_applied_segments = excluded.window_applied_segments,
                     window_applied_bytes = excluded.window_applied_bytes,
                     completed_window_started_at = excluded.completed_window_started_at,
                     completed_window_ended_at = excluded.completed_window_ended_at,
                     completed_generated_segments = excluded.completed_generated_segments,
                     completed_generated_bytes = excluded.completed_generated_bytes,
                     completed_acknowledged_segments = excluded.completed_acknowledged_segments,
                     completed_acknowledged_bytes = excluded.completed_acknowledged_bytes,
                     completed_applied_segments = excluded.completed_applied_segments,
                     completed_applied_bytes = excluded.completed_applied_bytes,
                     completed_peak_generated_segments = excluded.completed_peak_generated_segments,
                     completed_peak_generated_bytes = excluded.completed_peak_generated_bytes
                 WHERE repository_delivery_metric_scope_windows.window_started_at IS NULL
                   AND repository_delivery_metric_scope_windows.completed_window_started_at IS NULL
                   AND repository_delivery_metric_scope_windows.completed_window_ended_at IS NULL",
                rusqlite::params![source_node_id, stream, target_repository_id],
            )
            .map_err(sqlite_error)?;
    }
    transaction
        .execute(
            "INSERT INTO repository_delivery_metric_scope_windows
                 (source_node_id, stream, target_repository_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(source_node_id, stream, target_repository_id) DO NOTHING",
            rusqlite::params![source_node_id, stream, target_repository_id],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ScopeDeliveryWindowState {
    generated_segments: u64,
    generated_bytes: u64,
    peak_generated_segments: u64,
    peak_generated_bytes: u64,
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

fn read_scope_window_state(
    transaction: &rusqlite::Transaction<'_>,
    source_node_id: &str,
    stream: &str,
    target_repository_id: &str,
) -> Result<ScopeDeliveryWindowState> {
    transaction
        .query_row(
            "SELECT s.generated_segments, s.generated_bytes,
                    w.completed_peak_generated_segments,
                    w.completed_peak_generated_bytes,
                    s.acknowledged_segments, s.acknowledged_bytes,
                    s.applied_segments, s.applied_bytes,
                    w.window_started_at,
                    w.window_generated_segments, w.window_generated_bytes,
                    w.window_acknowledged_segments, w.window_acknowledged_bytes,
                    w.window_applied_segments, w.window_applied_bytes
             FROM repository_delivery_metric_scopes s
             JOIN repository_delivery_metric_scope_windows w
               ON w.source_node_id = s.source_node_id
              AND w.stream = s.stream
              AND w.target_repository_id = s.target_repository_id
             WHERE s.source_node_id = ?1 AND s.stream = ?2
               AND s.target_repository_id = ?3",
            rusqlite::params![source_node_id, stream, target_repository_id],
            |row| {
                Ok(ScopeDeliveryWindowState {
                    generated_segments: checked_u64(row.get::<_, i64>(0)?, 0)?,
                    generated_bytes: checked_u64(row.get::<_, i64>(1)?, 1)?,
                    peak_generated_segments: row
                        .get::<_, Option<i64>>(2)?
                        .map(|value| checked_u64(value, 2))
                        .transpose()?
                        .unwrap_or(0),
                    peak_generated_bytes: row
                        .get::<_, Option<i64>>(3)?
                        .map(|value| checked_u64(value, 3))
                        .transpose()?
                        .unwrap_or(0),
                    acknowledged_segments: checked_u64(row.get::<_, i64>(4)?, 4)?,
                    acknowledged_bytes: checked_u64(row.get::<_, i64>(5)?, 5)?,
                    applied_segments: checked_u64(row.get::<_, i64>(6)?, 6)?,
                    applied_bytes: checked_u64(row.get::<_, i64>(7)?, 7)?,
                    generated_segments_at_start: row
                        .get::<_, Option<i64>>(9)?
                        .map(|value| checked_u64(value, 9))
                        .transpose()?
                        .unwrap_or(0),
                    generated_bytes_at_start: row
                        .get::<_, Option<i64>>(10)?
                        .map(|value| checked_u64(value, 10))
                        .transpose()?
                        .unwrap_or(0),
                    acknowledged_segments_at_start: row
                        .get::<_, Option<i64>>(11)?
                        .map(|value| checked_u64(value, 11))
                        .transpose()?
                        .unwrap_or(0),
                    acknowledged_bytes_at_start: row
                        .get::<_, Option<i64>>(12)?
                        .map(|value| checked_u64(value, 12))
                        .transpose()?
                        .unwrap_or(0),
                    applied_segments_at_start: row
                        .get::<_, Option<i64>>(13)?
                        .map(|value| checked_u64(value, 13))
                        .transpose()?
                        .unwrap_or(0),
                    applied_bytes_at_start: row
                        .get::<_, Option<i64>>(14)?
                        .map(|value| checked_u64(value, 14))
                        .transpose()?
                        .unwrap_or(0),
                })
            },
        )
        .optional()
        .map_err(sqlite_error)?
        .ok_or_else(|| HistoryStorageError("delivery scope window row is missing".to_owned()))
}

#[allow(clippy::too_many_arguments)]
fn set_scope_window_baseline(
    transaction: &rusqlite::Transaction<'_>,
    source_node_id: &str,
    stream: &str,
    target_repository_id: &str,
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
            "UPDATE repository_delivery_metric_scope_windows
             SET window_started_at = ?1,
                 window_generated_segments = ?2,
                 window_generated_bytes = ?3,
                 window_acknowledged_segments = ?4,
                 window_acknowledged_bytes = ?5,
                 window_applied_segments = ?6,
                 window_applied_bytes = ?7
             WHERE source_node_id = ?8 AND stream = ?9 AND target_repository_id = ?10",
            rusqlite::params![
                durable_i64(started_at, "scope window start")?,
                durable_i64(generated_segments, "scope window generated segments")?,
                durable_i64(generated_bytes, "scope window generated bytes")?,
                durable_i64(acknowledged_segments, "scope window acknowledged segments")?,
                durable_i64(acknowledged_bytes, "scope window acknowledged bytes")?,
                durable_i64(applied_segments, "scope window applied segments")?,
                durable_i64(applied_bytes, "scope window applied bytes")?,
                source_node_id,
                stream,
                target_repository_id,
            ],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn rotate_scope_window(
    transaction: &rusqlite::Transaction<'_>,
    source_node_id: &str,
    stream: &str,
    target_repository_id: &str,
    progress_at: Option<u64>,
) -> Result<()> {
    let Some(progress_at) = progress_at else {
        return Ok(());
    };
    let window_started_at = transaction
        .query_row(
            "SELECT window_started_at
             FROM repository_delivery_metric_scope_windows
             WHERE source_node_id = ?1 AND stream = ?2 AND target_repository_id = ?3",
            rusqlite::params![source_node_id, stream, target_repository_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(sqlite_error)?
        .map(|value| checked_u64(value, 0))
        .transpose()
        .map_err(sqlite_error)?;
    let Some(window_started_at) = window_started_at else {
        let state =
            read_scope_window_state(transaction, source_node_id, stream, target_repository_id)?;
        return set_scope_window_baseline(
            transaction,
            source_node_id,
            stream,
            target_repository_id,
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
    let state = read_scope_window_state(transaction, source_node_id, stream, target_repository_id)?;
    let generated_segments = state
        .generated_segments
        .saturating_sub(state.generated_segments_at_start);
    let generated_bytes = state
        .generated_bytes
        .saturating_sub(state.generated_bytes_at_start);
    let peak_generated_segments = state.peak_generated_segments.max(generated_segments);
    let peak_generated_bytes = state.peak_generated_bytes.max(generated_bytes);
    let window = RepositoryDeliveryWindow::from_deltas_with_peak(
        window_started_at,
        window_started_at.saturating_add(DELIVERY_RATE_WINDOW_SECONDS),
        DELIVERY_RATE_WINDOW_SECONDS,
        generated_segments,
        generated_bytes,
        peak_generated_segments,
        peak_generated_bytes,
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
            "UPDATE repository_delivery_metric_scope_windows
             SET completed_window_started_at = ?1,
                 completed_window_ended_at = ?2,
                 completed_generated_segments = ?3,
                 completed_generated_bytes = ?4,
                 completed_acknowledged_segments = ?5,
                 completed_acknowledged_bytes = ?6,
                 completed_applied_segments = ?7,
                 completed_applied_bytes = ?8,
                 completed_peak_generated_segments = ?9,
                 completed_peak_generated_bytes = ?10
             WHERE source_node_id = ?11 AND stream = ?12 AND target_repository_id = ?13",
            rusqlite::params![
                durable_i64(window.started_at, "scope completed window start")?,
                durable_i64(window.ended_at, "scope completed window end")?,
                durable_i64(
                    window.generated_segments,
                    "scope completed generated segments"
                )?,
                durable_i64(window.generated_bytes, "scope completed generated bytes")?,
                durable_i64(
                    window.acknowledged_segments,
                    "scope completed acknowledged segments"
                )?,
                durable_i64(
                    window.acknowledged_bytes,
                    "scope completed acknowledged bytes"
                )?,
                durable_i64(window.applied_segments, "scope completed applied segments")?,
                durable_i64(window.applied_bytes, "scope completed applied bytes")?,
                durable_i64(peak_generated_segments, "scope peak generated segments")?,
                durable_i64(peak_generated_bytes, "scope peak generated bytes")?,
                source_node_id,
                stream,
                target_repository_id,
            ],
        )
        .map_err(sqlite_error)?;
    set_scope_window_baseline(
        transaction,
        source_node_id,
        stream,
        target_repository_id,
        progress_at,
        state.generated_segments,
        state.generated_bytes,
        state.acknowledged_segments,
        state.acknowledged_bytes,
        state.applied_segments,
        state.applied_bytes,
    )
}

pub(crate) fn increment_generated_scope(
    transaction: &rusqlite::Transaction<'_>,
    source_node_id: &str,
    stream: &str,
    segments: u64,
    bytes: u64,
    progress_at: Option<u64>,
) -> Result<()> {
    let mut targets = transaction
        .prepare(
            "SELECT target_repository_id
             FROM repository_delivery_metric_scopes
             WHERE source_node_id = ?1 AND stream = ?2
             ORDER BY target_repository_id",
        )
        .map_err(sqlite_error)?
        .query_map(rusqlite::params![source_node_id, stream], |row| {
            row.get::<_, String>(0)
        })
        .map_err(sqlite_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sqlite_error)?;
    if !targets.iter().any(|target| target.is_empty()) {
        targets.push(String::new());
    }
    for target_repository_id in targets {
        ensure_scope_row(
            transaction,
            source_node_id,
            stream,
            &target_repository_id,
            false,
        )?;
        rotate_scope_window(
            transaction,
            source_node_id,
            stream,
            &target_repository_id,
            progress_at,
        )?;
        add_scope_counters(
            transaction,
            source_node_id,
            stream,
            &target_repository_id,
            "generated_segments",
            segments,
            "generated_bytes",
            bytes,
        )?;
    }
    Ok(())
}

pub(crate) fn increment_acknowledged_scope(
    transaction: &rusqlite::Transaction<'_>,
    source_node_id: &str,
    stream: &str,
    target_repository_id: &str,
    segments: u64,
    bytes: u64,
    progress_at: Option<u64>,
) -> Result<()> {
    ensure_scope_row(
        transaction,
        source_node_id,
        stream,
        target_repository_id,
        true,
    )?;
    rotate_scope_window(
        transaction,
        source_node_id,
        stream,
        target_repository_id,
        progress_at,
    )?;
    add_scope_counters(
        transaction,
        source_node_id,
        stream,
        target_repository_id,
        "acknowledged_segments",
        segments,
        "acknowledged_bytes",
        bytes,
    )
}

pub(crate) fn increment_applied_scope(
    transaction: &rusqlite::Transaction<'_>,
    source_node_id: &str,
    stream: &str,
    target_repository_id: &str,
    segments: u64,
    bytes: u64,
    progress_at: Option<u64>,
) -> Result<()> {
    ensure_scope_row(
        transaction,
        source_node_id,
        stream,
        target_repository_id,
        true,
    )?;
    rotate_scope_window(
        transaction,
        source_node_id,
        stream,
        target_repository_id,
        progress_at,
    )?;
    add_scope_counters(
        transaction,
        source_node_id,
        stream,
        target_repository_id,
        "applied_segments",
        segments,
        "applied_bytes",
        bytes,
    )
}

pub(super) fn read_scopes(
    connection: &Connection,
    now_unix_seconds: Option<u64>,
) -> Result<Vec<RepositoryDeliveryScopeMetrics>> {
    let scopes = {
        let mut statement = connection
            .prepare(
                "SELECT source_node_id, stream, target_repository_id,
                        generated_segments, generated_bytes,
                        acknowledged_segments, acknowledged_bytes,
                        applied_segments, applied_bytes
                 FROM repository_delivery_metric_scopes
                 ORDER BY source_node_id, stream, target_repository_id",
            )
            .map_err(sqlite_error)?;
        statement
            .query_map([], |row| {
                Ok(RepositoryDeliveryScopeMetrics {
                    source_node_id: row.get(0)?,
                    stream: row.get(1)?,
                    target_repository_id: row.get(2)?,
                    generated_segments: checked_u64(row.get::<_, i64>(3)?, 3)?,
                    generated_bytes: checked_u64(row.get::<_, i64>(4)?, 4)?,
                    acknowledged_segments: checked_u64(row.get::<_, i64>(5)?, 5)?,
                    acknowledged_bytes: checked_u64(row.get::<_, i64>(6)?, 6)?,
                    applied_segments: checked_u64(row.get::<_, i64>(7)?, 7)?,
                    applied_bytes: checked_u64(row.get::<_, i64>(8)?, 8)?,
                    last_completed_window: None,
                })
            })
            .map_err(sqlite_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)?
    };
    scopes
        .into_iter()
        .map(|mut scope| {
            scope.last_completed_window = read_scope_window_at(
                connection,
                &scope.source_node_id,
                &scope.stream,
                &scope.target_repository_id,
                scope.generated_segments,
                scope.generated_bytes,
                scope.acknowledged_segments,
                scope.acknowledged_bytes,
                scope.applied_segments,
                scope.applied_bytes,
                now_unix_seconds,
            )?;
            Ok(scope)
        })
        .collect::<Result<Vec<_>>>()
}

#[allow(clippy::too_many_arguments)]
fn read_scope_window_at(
    connection: &Connection,
    source_node_id: &str,
    stream: &str,
    target_repository_id: &str,
    generated_segments: u64,
    generated_bytes: u64,
    acknowledged_segments: u64,
    acknowledged_bytes: u64,
    applied_segments: u64,
    applied_bytes: u64,
    now_unix_seconds: Option<u64>,
) -> Result<Option<RepositoryDeliveryWindow>> {
    let values = connection
        .query_row(
            "SELECT window_started_at,
                    window_generated_segments, window_generated_bytes,
                    window_acknowledged_segments, window_acknowledged_bytes,
                    window_applied_segments, window_applied_bytes,
                    completed_window_started_at, completed_window_ended_at,
                    completed_generated_segments, completed_generated_bytes,
                    completed_acknowledged_segments, completed_acknowledged_bytes,
                    completed_applied_segments, completed_applied_bytes,
                    completed_peak_generated_segments, completed_peak_generated_bytes
             FROM repository_delivery_metric_scope_windows
             WHERE source_node_id = ?1 AND stream = ?2 AND target_repository_id = ?3",
            rusqlite::params![source_node_id, stream, target_repository_id],
            |row| {
                (0..17)
                    .map(|index| row.get::<_, Option<i64>>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()
            },
        )
        .optional()
        .map_err(sqlite_error)?;
    let Some(values) = values else {
        return Ok(None);
    };
    let values = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| value.map(|value| checked_u64(value, index)).transpose())
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sqlite_error)?;
    let window = |started_at: Option<u64>,
                  ended_at: Option<u64>,
                  elapsed_seconds: u64,
                  generated_segments: u64,
                  generated_bytes: u64,
                  acknowledged_segments: u64,
                  acknowledged_bytes: u64,
                  applied_segments: u64,
                  applied_bytes: u64|
     -> Option<RepositoryDeliveryWindow> {
        Some(RepositoryDeliveryWindow::from_deltas_with_peak(
            started_at?,
            ended_at?,
            elapsed_seconds,
            generated_segments,
            generated_bytes,
            values[15].unwrap_or(generated_segments),
            values[16].unwrap_or(generated_bytes),
            acknowledged_segments,
            acknowledged_bytes,
            applied_segments,
            applied_bytes,
        ))
    };
    if let Some(completed) = window(
        values[7],
        values[8],
        DELIVERY_RATE_WINDOW_SECONDS,
        values[9].unwrap_or(0),
        values[10].unwrap_or(0),
        values[11].unwrap_or(0),
        values[12].unwrap_or(0),
        values[13].unwrap_or(0),
        values[14].unwrap_or(0),
    ) {
        return Ok(Some(completed));
    }
    let Some(now) = now_unix_seconds else {
        return Ok(None);
    };
    let Some(started_at) = values[0] else {
        return Ok(None);
    };
    if now < started_at.saturating_add(DELIVERY_RATE_WINDOW_SECONDS) {
        return Ok(None);
    }
    let peak_generated_segments = values[15]
        .unwrap_or(0)
        .max(generated_segments.saturating_sub(values[1].unwrap_or(0)));
    let peak_generated_bytes = values[16]
        .unwrap_or(0)
        .max(generated_bytes.saturating_sub(values[2].unwrap_or(0)));
    Ok(Some(RepositoryDeliveryWindow::from_deltas_with_peak(
        started_at,
        started_at.saturating_add(DELIVERY_RATE_WINDOW_SECONDS),
        DELIVERY_RATE_WINDOW_SECONDS,
        generated_segments.saturating_sub(values[1].unwrap_or(0)),
        generated_bytes.saturating_sub(values[2].unwrap_or(0)),
        peak_generated_segments,
        peak_generated_bytes,
        acknowledged_segments.saturating_sub(values[3].unwrap_or(0)),
        acknowledged_bytes.saturating_sub(values[4].unwrap_or(0)),
        applied_segments.saturating_sub(values[5].unwrap_or(0)),
        applied_bytes.saturating_sub(values[6].unwrap_or(0)),
    )))
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
