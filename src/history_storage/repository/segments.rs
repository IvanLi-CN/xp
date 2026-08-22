use super::*;

impl HistoryStorage {
    #[allow(dead_code)]
    pub(crate) fn upsert_repository_history_segments(
        &self,
        rows: &[RepositoryHistorySegmentRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Err(HistoryStorageError(
                "repository history row storage requires SQLite".to_owned(),
            ));
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        for row in rows {
            upsert_repository_history_segment(&transaction, row)?;
        }
        transaction.commit().map_err(sqlite_error)?;
        maintain_sqlite(connection)
    }

    /// A phase-prefixed keyset keeps every tombstone segment ahead of ordinary repair data while
    /// preserving source-cursor order within each phase.
    pub(crate) fn repository_history_segments_page(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RepositoryHistorySegmentRow>> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(Vec::new());
        };
        let (tombstones, after_id) = match after_id {
            Some(cursor) if cursor.starts_with("t:") => (true, Some(&cursor[2..])),
            Some(cursor) if cursor.starts_with("r:") => (false, Some(&cursor[2..])),
            Some(_) | None => (true, None),
        };
        let mut rows = segment_phase(connection, tombstones, after_id, limit)?;
        if tombstones && rows.len() < limit {
            rows.extend(segment_phase(connection, false, None, limit - rows.len())?);
        }
        Ok(rows)
    }

    pub(crate) fn repository_history_segments_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<RepositoryHistorySegmentRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(Vec::new());
        };
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id, closed_at, contains_tombstone, source_node_id, source_epoch, stream,
                    first_sequence, payload
             FROM repository_history_segments
             WHERE id IN ({placeholders}) ORDER BY id ASC"
        );
        let mut statement = connection.prepare(&sql).map_err(sqlite_error)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(ids.iter()), segment_row)
            .map_err(sqlite_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)
    }

    pub(crate) fn repository_history_segments_missing_cursor_index(
        &self,
        limit: usize,
    ) -> Result<Vec<RepositoryHistorySegmentRow>> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(Vec::new());
        };
        let mut statement = connection
            .prepare(
                "SELECT id, closed_at, contains_tombstone, source_node_id, source_epoch, stream,
                        first_sequence, payload
                 FROM repository_history_segments
                 WHERE source_node_id = ''
                 ORDER BY id ASC
                 LIMIT ?1",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], segment_row)
            .map_err(sqlite_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)
    }
}

fn segment_phase(
    connection: &Connection,
    tombstones: bool,
    after_id: Option<&str>,
    limit: usize,
) -> Result<Vec<RepositoryHistorySegmentRow>> {
    let mut statement = connection
        .prepare(
            "SELECT id, closed_at, contains_tombstone, source_node_id, source_epoch, stream,
                    first_sequence, payload
             FROM repository_history_segments
             WHERE contains_tombstone = ?1
               AND (
                    ?2 IS NULL
                    OR (source_node_id, source_epoch, stream, first_sequence, id) > (
                        SELECT source_node_id, source_epoch, stream, first_sequence, id
                        FROM repository_history_segments
                        WHERE id = ?2
                    )
               )
             ORDER BY source_node_id ASC, source_epoch ASC, stream ASC, first_sequence ASC,
                      id ASC
             LIMIT ?3",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map(
            params![
                tombstones,
                after_id,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            segment_row,
        )
        .map_err(sqlite_error)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sqlite_error)
}

fn segment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryHistorySegmentRow> {
    Ok(RepositoryHistorySegmentRow {
        id: row.get(0)?,
        closed_at_unix_seconds: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
        contains_tombstone: row.get(2)?,
        source_node_id: row.get(3)?,
        source_epoch: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(u64::MAX),
        stream: row.get(5)?,
        first_sequence: u64::try_from(row.get::<_, i64>(6)?).unwrap_or(u64::MAX),
        payload: row.get(7)?,
    })
}
