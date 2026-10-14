use super::source_journal::SOURCE_DELIVERY_JOURNAL_RESUME_PERCENT;
use super::*;
use crate::history_sync::SignedSegment;

fn canonical_metric_len(wire: &[u8]) -> u64 {
    SignedSegment::from_wire(wire)
        .ok()
        .and_then(|segment| segment.canonical().canonical_bytes().ok())
        .map_or_else(|| wire.len() as u64, |bytes| bytes.len() as u64)
}

pub(super) fn acknowledge_source_delivery_journal_inner(
    storage: &HistoryStorage,
    ids: &[String],
    acknowledged_at_unix_seconds: Option<u64>,
    delivery_path: Option<&str>,
    replay_stream_cursor: Option<&str>,
    control_payload: Option<&[u8]>,
    source_attempt: Option<metrics::SourceDeliveryAttempt>,
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut backend = storage.lock_backend();
    let Some(connection) = sqlite_connection(&mut backend)? else {
        return Err(HistoryStorageError(
            "source delivery journal requires SQLite".to_owned(),
        ));
    };
    let transaction = connection.transaction().map_err(sqlite_error)?;
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let select_sql = format!(
        "SELECT source_node_id, stream, wire
         FROM source_delivery_journal WHERE id IN ({placeholders})"
    );
    let acknowledged_rows = transaction
        .prepare(&select_sql)
        .map_err(sqlite_error)?
        .query_map(rusqlite::params_from_iter(ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(sqlite_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sqlite_error)?;
    let acknowledged_segments = u64::try_from(acknowledged_rows.len())
        .map_err(|_| HistoryStorageError("acknowledged segment count exceeds u64".to_owned()))?;
    let acknowledged_wire_bytes =
        acknowledged_rows
            .iter()
            .try_fold(0_u64, |total, (_, _, wire)| {
                total
                    .checked_add(u64::try_from(wire.len()).map_err(|_| {
                        HistoryStorageError("acknowledged wire length exceeds u64".to_owned())
                    })?)
                    .ok_or_else(|| {
                        HistoryStorageError("acknowledged wire byte count overflow".to_owned())
                    })
            })?;
    let acknowledged_canonical_bytes =
        acknowledged_rows
            .iter()
            .try_fold(0_u64, |total, (_, _, wire)| {
                total
                    .checked_add(canonical_metric_len(wire))
                    .ok_or_else(|| {
                        HistoryStorageError("acknowledged canonical byte count overflow".to_owned())
                    })
            })?;
    let deleted_any = acknowledged_segments > 0;
    if deleted_any {
        let delete_sql =
            format!("DELETE FROM source_delivery_journal WHERE id IN ({placeholders})");
        let deleted = transaction
            .execute(&delete_sql, rusqlite::params_from_iter(ids.iter()))
            .map_err(sqlite_error)?;
        if u64::try_from(deleted).ok() != Some(acknowledged_segments) {
            return Err(HistoryStorageError(
                "source delivery journal acknowledgement changed during transaction".to_owned(),
            ));
        }
        transaction
            .execute(
                "UPDATE source_delivery_journal_state
                 SET pending_segments = pending_segments - ?1,
                     pending_bytes = pending_bytes - ?2
                 WHERE singleton = 1",
                rusqlite::params![
                    i64::try_from(acknowledged_segments).map_err(|_| {
                        HistoryStorageError(
                            "acknowledged segment count exceeds SQLite integer".to_owned(),
                        )
                    })?,
                    i64::try_from(acknowledged_wire_bytes).map_err(|_| {
                        HistoryStorageError(
                            "acknowledged byte count exceeds SQLite integer".to_owned(),
                        )
                    })?,
                ],
            )
            .map_err(sqlite_error)?;
        let mut stream_counts = std::collections::BTreeMap::<String, u64>::new();
        for (_, stream, _) in &acknowledged_rows {
            *stream_counts.entry(stream.clone()).or_default() += 1;
        }
        let target_repository_id = source_attempt
            .as_ref()
            .and_then(|attempt| attempt.target_repository_id.as_deref())
            .or(delivery_path)
            .unwrap_or("unknown");
        let mut scope_counts = std::collections::BTreeMap::<(String, String), (u64, u64)>::new();
        for (source_node_id, stream, wire) in &acknowledged_rows {
            let entry = scope_counts
                .entry((source_node_id.clone(), stream.clone()))
                .or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(canonical_metric_len(wire));
        }
        for ((source_node_id, stream), (segments, bytes)) in scope_counts {
            metrics::increment_acknowledged_scope(
                &transaction,
                &source_node_id,
                &stream,
                target_repository_id,
                segments,
                bytes,
                acknowledged_at_unix_seconds.or_else(|| {
                    source_attempt
                        .as_ref()
                        .map(|attempt| attempt.now_unix_seconds)
                }),
            )?;
        }
        for (stream, count) in stream_counts {
            transaction
                .execute(
                    "UPDATE source_delivery_journal_stream_state
                     SET pending_segments = pending_segments - ?1
                     WHERE stream = ?2",
                    rusqlite::params![
                        i64::try_from(count).map_err(|_| {
                            HistoryStorageError(
                                "stream acknowledgement count exceeds SQLite integer".to_owned(),
                            )
                        })?,
                        stream,
                    ],
                )
                .map_err(sqlite_error)?;
        }
    }
    transaction
        .execute(
            "UPDATE source_delivery_journal_state
             SET capacity_suspended = CASE
                 WHEN pending_segments * 100 < ?1 * ?3
                  AND pending_bytes * 100 < ?2 * ?3 THEN 0
                 ELSE capacity_suspended END
             WHERE singleton = 1",
            rusqlite::params![
                SOURCE_DELIVERY_JOURNAL_MAX_SEGMENTS as i64,
                SOURCE_DELIVERY_JOURNAL_MAX_BYTES as i64,
                SOURCE_DELIVERY_JOURNAL_RESUME_PERCENT,
            ],
        )
        .map_err(sqlite_error)?;
    if deleted_any && let Some(acknowledged_at) = acknowledged_at_unix_seconds {
        transaction
            .execute(
                "UPDATE source_delivery_journal_state
                 SET last_acknowledged_at = ?1,
                     last_delivery_path = COALESCE(?2, last_delivery_path)
                 WHERE singleton = 1",
                params![
                    durable_i64(acknowledged_at, "journal acknowledgement time")?,
                    delivery_path,
                ],
            )
            .map_err(sqlite_error)?;
    }
    if deleted_any {
        metrics::increment_acknowledged(
            &transaction,
            acknowledged_segments,
            acknowledged_canonical_bytes,
            acknowledged_at_unix_seconds,
        )?;
    }
    if let Some(attempt) = source_attempt {
        metrics::record_source_attempt(&transaction, attempt)?;
    }
    if deleted_any && let Some(stream) = replay_stream_cursor {
        transaction
            .execute(
                "UPDATE source_delivery_journal_state
                 SET replay_stream_cursor = ?1
                 WHERE singleton = 1",
                [stream],
            )
            .map_err(sqlite_error)?;
    }
    if deleted_any && let Some(control_payload) = control_payload {
        super::write_snapshot(&transaction, REPOSITORY_REPLICA_KEY, control_payload)?;
    }
    transaction.commit().map_err(sqlite_error)?;
    finish_post_commit_maintenance(maintain_sqlite(connection));
    Ok(())
}
