use super::*;

pub(super) fn insert_journal_rows(
    transaction: &rusqlite::Transaction<'_>,
    rows: &[SourceDeliveryJournalRow],
) -> Result<()> {
    let (pending_segments, pending_bytes) = transaction
        .query_row(
            "SELECT pending_segments, pending_bytes
             FROM source_delivery_journal_state WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(sqlite_error)?;
    let mut projected_segments = pending_segments;
    let mut projected_bytes = pending_bytes;
    let mut projected_rows = std::collections::BTreeMap::<String, Option<(String, i64)>>::new();
    for row in rows {
        let wire_len = i64::try_from(row.wire.len()).map_err(|_| {
            HistoryStorageError("journal wire length exceeds SQLite integer".to_owned())
        })?;
        SignedSegment::from_wire(&row.wire).map_err(|error| {
            HistoryStorageError(format!("invalid source delivery journal wire: {error}"))
        })?;
        let previous_row = if let Some(previous) = projected_rows.get(&row.id) {
            previous.clone()
        } else {
            transaction
                .query_row(
                    "SELECT stream, length(wire)
                     FROM source_delivery_journal WHERE id = ?1",
                    [&row.id],
                    |query_row| Ok((query_row.get::<_, String>(0)?, query_row.get::<_, i64>(1)?)),
                )
                .optional()
                .map_err(sqlite_error)?
        };
        match previous_row.as_ref().map(|(_, wire_len)| *wire_len) {
            Some(previous_wire_len) => {
                projected_bytes = projected_bytes
                    .checked_sub(previous_wire_len)
                    .and_then(|value| value.checked_add(wire_len))
                    .ok_or_else(|| HistoryStorageError("journal byte count overflow".to_owned()))?;
            }
            None => {
                projected_segments = projected_segments.checked_add(1).ok_or_else(|| {
                    HistoryStorageError("journal segment count overflow".to_owned())
                })?;
                projected_bytes = projected_bytes
                    .checked_add(wire_len)
                    .ok_or_else(|| HistoryStorageError("journal byte count overflow".to_owned()))?;
            }
        }
        projected_rows.insert(row.id.clone(), Some((row.stream.clone(), wire_len)));
        if projected_segments > SOURCE_DELIVERY_JOURNAL_MAX_SEGMENTS as i64
            || projected_bytes > SOURCE_DELIVERY_JOURNAL_MAX_BYTES as i64
        {
            return Err(HistoryStorageError(
                "source delivery journal capacity guard".to_owned(),
            ));
        }
    }
    for row in rows {
        let identity = serde_json::to_vec(&row.identity)
            .map_err(|error| HistoryStorageError(error.to_string()))?;
        let segment = SignedSegment::from_wire(&row.wire).map_err(|error| {
            HistoryStorageError(format!("invalid source delivery journal wire: {error}"))
        })?;
        let cursor = segment.canonical().first_cursor();
        let previous_row = transaction
            .query_row(
                "SELECT stream, length(wire)
                 FROM source_delivery_journal WHERE id = ?1",
                [&row.id],
                |query_row| Ok((query_row.get::<_, String>(0)?, query_row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "INSERT INTO source_delivery_journal
                     (id, stream, closed_at, identity, wire, created_at,
                      source_node_id, source_epoch, first_sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                     stream = excluded.stream,
                     closed_at = excluded.closed_at,
                     identity = excluded.identity,
                     wire = excluded.wire,
                     source_node_id = excluded.source_node_id,
                     source_epoch = excluded.source_epoch,
                     first_sequence = excluded.first_sequence",
                params![
                    row.id,
                    row.stream,
                    durable_i64(row.closed_at_unix_seconds, "journal closed_at")?,
                    identity,
                    row.wire,
                    durable_i64(row.closed_at_unix_seconds, "journal created_at")?,
                    cursor.source_node_id(),
                    durable_i64(cursor.source_epoch(), "journal source epoch")?,
                    durable_i64(cursor.sequence(), "journal first sequence")?,
                ],
            )
            .map_err(sqlite_error)?;
        let wire_len = i64::try_from(row.wire.len()).map_err(|_| {
            HistoryStorageError("journal wire length exceeds SQLite integer".to_owned())
        })?;
        match previous_row {
            Some((previous_stream, previous_wire_len)) => {
                let delta = wire_len.checked_sub(previous_wire_len).ok_or_else(|| {
                    HistoryStorageError("journal wire length delta overflow".to_owned())
                })?;
                transaction
                    .execute(
                        "UPDATE source_delivery_journal_state
                         SET pending_bytes = pending_bytes + ?1,
                             epoch_high_water = MAX(epoch_high_water, ?2),
                             capacity_suspended = CASE
                               WHEN (pending_segments * 100 >= ?3 * ?4)
                                 OR ((pending_bytes + ?1) * 100 >= ?5 * ?4)
                               THEN 1 ELSE capacity_suspended END
                         WHERE singleton = 1",
                        params![
                            delta,
                            durable_i64(cursor.source_epoch(), "journal source epoch")?,
                            SOURCE_DELIVERY_JOURNAL_MAX_SEGMENTS as i64,
                            SOURCE_DELIVERY_JOURNAL_SUSPEND_PERCENT,
                            SOURCE_DELIVERY_JOURNAL_MAX_BYTES as i64,
                        ],
                    )
                    .map_err(sqlite_error)?;
                if previous_stream != row.stream {
                    transaction
                        .execute(
                            "UPDATE source_delivery_journal_stream_state
                             SET pending_segments = pending_segments - 1
                             WHERE stream = ?1",
                            [&previous_stream],
                        )
                        .map_err(sqlite_error)?;
                    transaction
                        .execute(
                            "INSERT INTO source_delivery_journal_stream_state
                                 (stream, pending_segments) VALUES (?1, 1)
                             ON CONFLICT(stream) DO UPDATE SET
                                 pending_segments = pending_segments + 1",
                            [&row.stream],
                        )
                        .map_err(sqlite_error)?;
                }
            }
            None => {
                transaction
                    .execute(
                        "UPDATE source_delivery_journal_state
                         SET pending_segments = pending_segments + 1,
                             pending_bytes = pending_bytes + ?1,
                             epoch_high_water = MAX(epoch_high_water, ?2),
                             capacity_suspended = CASE
                               WHEN ((pending_segments + 1) * 100 >= ?3 * ?4)
                                 OR ((pending_bytes + ?1) * 100 >= ?5 * ?4)
                               THEN 1 ELSE capacity_suspended END
                         WHERE singleton = 1",
                        params![
                            wire_len,
                            durable_i64(cursor.source_epoch(), "journal source epoch")?,
                            SOURCE_DELIVERY_JOURNAL_MAX_SEGMENTS as i64,
                            SOURCE_DELIVERY_JOURNAL_SUSPEND_PERCENT,
                            SOURCE_DELIVERY_JOURNAL_MAX_BYTES as i64,
                        ],
                    )
                    .map_err(sqlite_error)?;
                transaction
                    .execute(
                        "INSERT INTO source_delivery_journal_stream_state
                             (stream, pending_segments) VALUES (?1, 1)
                         ON CONFLICT(stream) DO UPDATE SET
                             pending_segments = pending_segments + 1",
                        [&row.stream],
                    )
                    .map_err(sqlite_error)?;
            }
        }
    }
    Ok(())
}
