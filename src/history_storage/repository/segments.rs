use std::collections::BTreeSet;

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
        let connection = match &mut *backend {
            Backend::Sqlite(connection) => connection,
            Backend::Unavailable(error) => return Err(error.clone()),
            Backend::Json => {
                return Err(HistoryStorageError(
                    "repository history row storage requires SQLite".to_owned(),
                ));
            }
        };
        let transaction = connection.transaction().map_err(sqlite_error)?;
        let mut changed_sources = BTreeSet::new();
        for row in rows {
            if let Some(change) = upsert_repository_history_segment(&transaction, row)? {
                changed_sources.insert(row.source_node_id.clone());
                if let Some(previous_source_node_id) = change.previous_source_node_id {
                    changed_sources.insert(previous_source_node_id);
                }
            }
        }
        super::super::counts::bump_repository_history_segment_revisions(
            &transaction,
            &changed_sources,
        )?;
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
        self.repository_history_segments_page_for_source(after_id, None, limit)
    }

    pub(crate) fn repository_history_segments_page_for_source(
        &self,
        after_id: Option<&str>,
        source_node_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RepositoryHistorySegmentRow>> {
        let mut backend = self.lock_backend();
        let connection = match &mut *backend {
            Backend::Sqlite(connection) => connection,
            Backend::Unavailable(error) => return Err(error.clone()),
            Backend::Json => return Ok(Vec::new()),
        };
        segment_page_for_source(
            connection,
            after_id,
            limit,
            FULL_SEGMENT_PROJECTION,
            None,
            source_node_id,
            segment_row,
        )
    }

    pub(crate) fn repository_history_segment_metadata_page(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RepositoryHistorySegmentMetadataRow>> {
        self.repository_history_segment_metadata_page_for_source(after_id, None, limit)
    }

    pub(crate) fn repository_history_segment_metadata_page_for_source(
        &self,
        after_id: Option<&str>,
        source_node_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RepositoryHistorySegmentMetadataRow>> {
        let mut backend = self.lock_backend();
        let connection = match &mut *backend {
            Backend::Sqlite(connection) => connection,
            Backend::Unavailable(error) => return Err(error.clone()),
            Backend::Json => return Ok(Vec::new()),
        };
        segment_page_for_source(
            connection,
            after_id,
            limit,
            "id, contains_tombstone",
            Some("INDEXED BY repository_history_segments_sync_order_v2"),
            source_node_id,
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
        let connection = match &mut *backend {
            Backend::Sqlite(connection) => connection,
            Backend::Unavailable(error) => return Err(error.clone()),
            Backend::Json => return Ok(Vec::new()),
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

    pub(crate) fn repository_history_segment_first_sequences(
        &self,
        streams: &[(String, u64, String)],
    ) -> Result<Vec<(String, u64, String, u64)>> {
        if streams.is_empty() {
            return Ok(Vec::new());
        }
        let mut backend = self.lock_backend();
        let connection = match &mut *backend {
            Backend::Sqlite(connection) => connection,
            Backend::Unavailable(error) => return Err(error.clone()),
            Backend::Json => return Ok(Vec::new()),
        };
        streams
            .iter()
            .filter_map(|(source_node_id, source_epoch, stream)| {
                connection
                    .query_row(
                        "SELECT source_node_id, source_epoch, stream, MIN(first_sequence)
                         FROM repository_history_segments
                         WHERE source_node_id = ?1 AND source_epoch = ?2 AND stream = ?3
                         GROUP BY source_node_id, source_epoch, stream",
                        params![
                            source_node_id,
                            i64::try_from(*source_epoch).unwrap_or(i64::MAX),
                            stream
                        ],
                        |row| {
                            Ok((
                                row.get(0)?,
                                u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
                                row.get(2)?,
                                u64::try_from(row.get::<_, i64>(3)?).unwrap_or(u64::MAX),
                            ))
                        },
                    )
                    .optional()
                    .transpose()
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)
    }

    pub(crate) fn repository_history_segment_ids_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut backend = self.lock_backend();
        let connection = match &mut *backend {
            Backend::Sqlite(connection) => connection,
            Backend::Unavailable(error) => return Err(error.clone()),
            Backend::Json => return Ok(Vec::new()),
        };
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id FROM repository_history_segments
             WHERE id IN ({placeholders}) ORDER BY id ASC"
        );
        let mut statement = connection.prepare(&sql).map_err(sqlite_error)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(ids.iter()), |row| row.get(0))
            .map_err(sqlite_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error)
    }

    pub(crate) fn repository_history_segment_exists(&self, id: &str) -> Result<bool> {
        let mut backend = self.lock_backend();
        let connection = match &mut *backend {
            Backend::Sqlite(connection) => connection,
            Backend::Unavailable(error) => return Err(error.clone()),
            Backend::Json => return Ok(false),
        };
        connection
            .query_row(
                "SELECT EXISTS (
                     SELECT 1 FROM repository_history_segments WHERE id = ?1
                 )",
                [id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)
    }

    pub(crate) fn repository_history_segments_missing_cursor_index(
        &self,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RepositoryHistorySegmentRow>> {
        let mut backend = self.lock_backend();
        let connection = match &mut *backend {
            Backend::Sqlite(connection) => connection,
            Backend::Unavailable(error) => return Err(error.clone()),
            Backend::Json => return Ok(Vec::new()),
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

fn segment_page_for_source<T, F>(
    connection: &Connection,
    after_id: Option<&str>,
    limit: usize,
    projection: &str,
    index_hint: Option<&str>,
    source_node_id: Option<&str>,
    mut row_mapper: F,
) -> Result<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let spec = SegmentPageSpec {
        projection,
        index_hint,
        source_node_id,
    };
    let (mut tombstones, mut after_id, mut cursor_is_source_scoped) = match after_id {
        Some(cursor) if cursor.starts_with("s:t:") => (true, Some(&cursor[4..]), true),
        Some(cursor) if cursor.starts_with("s:r:") => (false, Some(&cursor[4..]), true),
        Some(cursor) if cursor.starts_with("t:") => (true, Some(&cursor[2..]), false),
        Some(cursor) if cursor.starts_with("r:") => (false, Some(&cursor[2..]), false),
        Some(_) | None => (true, None, false),
    };
    if let Some(after_cursor) = after_id {
        let cursor_matches = match source_node_id {
            Some(source_node_id) => {
                cursor_is_source_scoped
                    && connection
                        .query_row(
                            "SELECT contains_tombstone
                             FROM repository_history_segments
                             WHERE id = ?1 AND source_node_id = ?2",
                            params![after_cursor, source_node_id],
                            |row| row.get::<_, bool>(0),
                        )
                        .optional()
                        .map_err(sqlite_error)?
                        .is_some_and(|contains_tombstone| contains_tombstone == tombstones)
            }
            None => {
                !cursor_is_source_scoped
                    && connection
                        .query_row(
                            "SELECT contains_tombstone
                             FROM repository_history_segments
                             WHERE id = ?1",
                            [after_cursor],
                            |row| row.get::<_, bool>(0),
                        )
                        .optional()
                        .map_err(sqlite_error)?
                        .is_some_and(|contains_tombstone| contains_tombstone == tombstones)
            }
        };
        if !cursor_matches {
            tombstones = true;
            after_id = None;
            cursor_is_source_scoped = true;
        }
    }
    let mut rows = segment_phase_with_row(
        connection,
        tombstones,
        after_id,
        cursor_is_source_scoped,
        limit,
        &spec,
        &mut row_mapper,
    )?;
    if tombstones && rows.len() < limit {
        rows.extend(segment_phase_with_row(
            connection,
            false,
            None,
            true,
            limit - rows.len(),
            &spec,
            &mut row_mapper,
        )?);
    }
    Ok(rows)
}

struct SegmentPageSpec<'a> {
    projection: &'a str,
    index_hint: Option<&'a str>,
    source_node_id: Option<&'a str>,
}

fn segment_phase_with_row<T, F>(
    connection: &Connection,
    tombstones: bool,
    after_id: Option<&str>,
    cursor_is_source_scoped: bool,
    limit: usize,
    spec: &SegmentPageSpec<'_>,
    mut row_mapper: F,
) -> Result<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let page_limit = limit;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    if let Some(after_id) = after_id {
        if spec.source_node_id.is_some() && !cursor_is_source_scoped {
            // Legacy t:/r: cursors came from the unscoped inventory. They are not a proof of
            // source-scoped continuation, even when the retained row happens to share the source.
            return segment_phase_with_row(
                connection, true, None, true, page_limit, spec, row_mapper,
            );
        }
        let cursor = if let Some(source_node_id) = spec.source_node_id {
            connection.query_row(
                "SELECT source_node_id, contains_tombstone, source_epoch, stream, first_sequence, id
                 FROM repository_history_segments
                 WHERE id = ?1 AND source_node_id = ?2",
                params![after_id, source_node_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
        } else {
            connection.query_row(
                "SELECT source_node_id, contains_tombstone, source_epoch, stream, first_sequence, id
                 FROM repository_history_segments
                 WHERE id = ?1",
                [after_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
        }
        .optional()
        .map_err(sqlite_error)?;
        let Some((
            cursor_source_node_id,
            cursor_contains_tombstone,
            source_epoch,
            stream,
            first_sequence,
            id,
        )) = cursor
        else {
            // A cursor from the old unscoped inventory, or a retained row removed after the
            // checkpoint, cannot safely continue inside this source. Restart the source inventory
            // from its tombstone-first bound; callers reapply returned wires idempotently without
            // advancing the generation.
            return segment_phase_with_row(
                connection, true, None, true, page_limit, spec, row_mapper,
            );
        };
        if cursor_contains_tombstone != tombstones {
            // A phase prefix is bound to the row that produced it. A mismatched prefix is not a
            // safe keyset position; restart the source inventory from its tombstone-first bound.
            return segment_phase_with_row(
                connection, true, None, true, page_limit, spec, row_mapper,
            );
        }
        let sql = if spec.source_node_id.is_some() {
            segment_phase_sql_for_source(spec.projection, spec.index_hint, true)
        } else {
            segment_phase_sql(spec.projection, spec.index_hint, true)
        };
        let mut statement = connection.prepare(&sql).map_err(sqlite_error)?;
        let rows = if let Some(source_node_id) = spec.source_node_id {
            statement.query_map(
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
        } else {
            statement.query_map(
                params![
                    tombstones,
                    cursor_source_node_id,
                    source_epoch,
                    stream,
                    first_sequence,
                    id,
                    limit
                ],
                &mut row_mapper,
            )
        }
        .map_err(sqlite_error)?;
        return rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sqlite_error);
    }

    let sql = if spec.source_node_id.is_some() {
        segment_phase_sql_for_source(spec.projection, spec.index_hint, false)
    } else {
        segment_phase_sql(spec.projection, spec.index_hint, false)
    };
    let mut statement = connection.prepare(&sql).map_err(sqlite_error)?;
    let rows = if let Some(source_node_id) = spec.source_node_id {
        statement.query_map(params![tombstones, source_node_id, limit], &mut row_mapper)
    } else {
        statement.query_map(params![tombstones, limit], &mut row_mapper)
    }
    .map_err(sqlite_error)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sqlite_error)
}

pub(crate) fn segment_phase_sql(
    projection: &str,
    index_hint: Option<&str>,
    continuation: bool,
) -> String {
    let index_hint = index_hint.unwrap_or_default();
    if continuation {
        return format!(
            "SELECT {projection}
             FROM repository_history_segments {index_hint}
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
         FROM repository_history_segments {index_hint}
         WHERE contains_tombstone = ?1
         ORDER BY source_node_id ASC, source_epoch ASC, stream ASC, first_sequence ASC,
                  id ASC
         LIMIT ?2"
    )
}

fn segment_phase_sql_for_source(
    projection: &str,
    index_hint: Option<&str>,
    continuation: bool,
) -> String {
    let index_hint = index_hint.unwrap_or_default();
    if continuation {
        return format!(
            "SELECT {projection}
             FROM repository_history_segments {index_hint}
             WHERE contains_tombstone = ?1
               AND source_node_id = ?2
               AND (source_node_id, source_epoch, stream, first_sequence, id)
                   > (?2, ?3, ?4, ?5, ?6)
             ORDER BY source_node_id ASC, source_epoch ASC, stream ASC, first_sequence ASC,
                      id ASC
             LIMIT ?7"
        );
    }

    format!(
        "SELECT {projection}
         FROM repository_history_segments {index_hint}
         WHERE contains_tombstone = ?1
           AND source_node_id = ?2
         ORDER BY source_node_id ASC, source_epoch ASC, stream ASC, first_sequence ASC,
                  id ASC
         LIMIT ?3"
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
