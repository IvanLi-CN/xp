use super::*;

const FULL_SEGMENT_PROJECTION: &str = concat!(
    "id, closed_at, contains_tombstone, source_node_id, source_epoch, ",
    "stream, first_sequence, payload",
);

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
        segment_page(
            connection,
            after_id,
            limit,
            FULL_SEGMENT_PROJECTION,
            segment_row,
        )
    }

    pub(crate) fn repository_history_segment_metadata_page(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RepositoryHistorySegmentMetadataRow>> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(Vec::new());
        };
        segment_page(
            connection,
            after_id,
            limit,
            "id, contains_tombstone",
            metadata_segment_row,
        )
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
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RepositoryHistorySegmentRow>> {
        let mut backend = self.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            return Ok(Vec::new());
        };
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = if let Some(after_id) = after_id {
            let mut statement = connection
                .prepare(
                    "SELECT id, closed_at, contains_tombstone, source_node_id, source_epoch, stream,
                            first_sequence, payload
                     FROM repository_history_segments
                     WHERE source_node_id = '' AND id > ?1
                     ORDER BY id ASC
                     LIMIT ?2",
                )
                .map_err(sqlite_error)?;
            statement
                .query_map(params![after_id, limit], segment_row)
                .map_err(sqlite_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sqlite_error)?
        } else {
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
            statement
                .query_map([limit], segment_row)
                .map_err(sqlite_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sqlite_error)?
        };
        Ok(rows)
    }
}

fn segment_page<T, F>(
    connection: &Connection,
    after_id: Option<&str>,
    limit: usize,
    projection: &str,
    mut row_mapper: F,
) -> Result<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let (tombstones, after_id) = match after_id {
        Some(cursor) if cursor.starts_with("t:") => (true, Some(&cursor[2..])),
        Some(cursor) if cursor.starts_with("r:") => (false, Some(&cursor[2..])),
        Some(_) | None => (true, None),
    };
    let mut rows = segment_phase_with_row(
        connection,
        tombstones,
        after_id,
        limit,
        projection,
        &mut row_mapper,
    )?;
    if tombstones && rows.len() < limit {
        rows.extend(segment_phase_with_row(
            connection,
            false,
            None,
            limit - rows.len(),
            projection,
            &mut row_mapper,
        )?);
    }
    Ok(rows)
}

fn segment_phase_with_row<T, F>(
    connection: &Connection,
    tombstones: bool,
    after_id: Option<&str>,
    limit: usize,
    projection: &str,
    mut row_mapper: F,
) -> Result<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    if let Some(after_id) = after_id {
        let Some((source_node_id, source_epoch, stream, first_sequence, id)) = connection
            .query_row(
                "SELECT source_node_id, source_epoch, stream, first_sequence, id
                 FROM repository_history_segments
                 WHERE id = ?1",
                [after_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(sqlite_error)?
        else {
            return Ok(Vec::new());
        };
        let sql = segment_phase_sql(projection, true);
        let mut statement = connection.prepare(&sql).map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                params![
                    tombstones,
                    source_node_id,
                    source_epoch,
                    stream,
                    first_sequence,
                    id,
                    limit
                ],
                &mut row_mapper,
            )
            .map_err(sqlite_error)?;
        return rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error);
    }

    let sql = segment_phase_sql(projection, false);
    let mut statement = connection.prepare(&sql).map_err(sqlite_error)?;
    let rows = statement
        .query_map(params![tombstones, limit], &mut row_mapper)
        .map_err(sqlite_error)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sqlite_error)
}

pub(crate) fn segment_phase_sql(projection: &str, continuation: bool) -> String {
    if continuation {
        return format!(
            "SELECT {projection}
             FROM repository_history_segments
             WHERE contains_tombstone = ?1
               AND (source_node_id, source_epoch, stream, first_sequence, id)
                   > (?2, ?3, ?4, ?5, ?6)
             ORDER BY source_node_id ASC, source_epoch ASC, stream ASC, first_sequence ASC,
                      id ASC
             LIMIT ?7"
        );
    }

    format!(
        "SELECT {projection}
         FROM repository_history_segments
         WHERE contains_tombstone = ?1
         ORDER BY source_node_id ASC, source_epoch ASC, stream ASC, first_sequence ASC,
                  id ASC
         LIMIT ?2"
    )
}

fn metadata_segment_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RepositoryHistorySegmentMetadataRow> {
    Ok(RepositoryHistorySegmentMetadataRow {
        id: row.get(0)?,
        contains_tombstone: row.get(1)?,
    })
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
