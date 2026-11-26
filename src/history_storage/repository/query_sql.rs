use super::*;

/// Decode the four-column coverage projection shared by both coverage paths.
pub(super) fn coverage_row(
    row: &rusqlite::Row<'_>,
    observed_index: usize,
) -> rusqlite::Result<Option<RepositoryHistoryCoverage>> {
    let observed_start = row.get::<_, Option<i64>>(observed_index)?;
    let Some(observed_start) = observed_start else {
        return Ok(None);
    };
    Ok(Some(RepositoryHistoryCoverage {
        observed_start_unix_seconds: u64::try_from(observed_start).unwrap_or(u64::MAX),
        observed_end_unix_seconds: u64::try_from(row.get::<_, i64>(observed_index + 1)?)
            .unwrap_or(u64::MAX),
        received_start_unix_seconds: u64::try_from(row.get::<_, i64>(observed_index + 2)?)
            .unwrap_or(u64::MAX),
        received_end_unix_seconds: u64::try_from(row.get::<_, i64>(observed_index + 3)?)
            .unwrap_or(u64::MAX),
    }))
}

/// Whole-repository coverage from four independent index boundary seeks.
///
/// `MIN(observed_start)`, `MAX(observed_end)`, `MIN(received_at)` and `MAX(received_at)` each read
/// one boundary of an index whose prefix is `is_tombstone`, so none of them touches retained
/// payload or walks the table. A single four-column aggregate cannot do that: it must visit every
/// index entry, and its cost grows with the repository instead of staying constant.
pub(super) fn repository_history_coverage_boundaries(
    connection: &Connection,
) -> Result<Option<super::RepositoryHistoryCoverage>> {
    let observed_start = connection
        .query_row(
            "SELECT MIN(observed_start) FROM repository_history_records
             INDEXED BY repository_history_records_keyset WHERE is_tombstone = 0",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(sqlite_error)?;
    let Some(observed_start) = observed_start else {
        return Ok(None);
    };
    let observed_end = connection
        .query_row(
            "SELECT MAX(observed_end) FROM repository_history_records
             INDEXED BY repository_history_records_export_filter WHERE is_tombstone = 0",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sqlite_error)?;
    let received_start = connection
        .query_row(
            "SELECT MIN(received_at) FROM repository_history_records
             INDEXED BY repository_history_records_received WHERE is_tombstone = 0",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sqlite_error)?;
    let received_end = connection
        .query_row(
            "SELECT MAX(received_at) FROM repository_history_records
             INDEXED BY repository_history_records_received WHERE is_tombstone = 0",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sqlite_error)?;
    Ok(Some(RepositoryHistoryCoverage {
        observed_start_unix_seconds: u64::try_from(observed_start).unwrap_or(u64::MAX),
        observed_end_unix_seconds: u64::try_from(observed_end).unwrap_or(u64::MAX),
        received_start_unix_seconds: u64::try_from(received_start).unwrap_or(u64::MAX),
        received_end_unix_seconds: u64::try_from(received_end).unwrap_or(u64::MAX),
    }))
}

/// Select one bounded record page with predicates SQLite can push into an index seek.
///
/// Filter dimensions that may be absent choose a branch with literal predicates instead of a
/// `(?N IS NULL OR column ...)` form, because that form makes the planner drop every later term and
/// scan the whole index. A present time window always contributes an indexed `observed_start`
/// lower bound widened by [`MAX_OBSERVED_SPAN_SECONDS`]; the exact `observed_end` predicate still
/// decides each row, so the returned set is unchanged.
pub(crate) fn repository_history_records_sql(
    subject_node_id: Option<&str>,
    schema_id: Option<&str>,
    start_unix_seconds: Option<u64>,
) -> &'static str {
    match (subject_node_id, schema_id, start_unix_seconds.is_some()) {
        (Some(_), Some(_), true) => {
            "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_page
        WHERE is_tombstone = 0 AND subject_node_id = ?1 AND schema_id = ?2
          AND observed_start >= ?3 - ?7 AND observed_start <= ?4 AND observed_end >= ?3
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT ?5 OFFSET ?6"
        }
        (Some(_), Some(_), false) => {
            "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_query
        WHERE is_tombstone = 0 AND subject_node_id = ?1 AND schema_id = ?2
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT ?5 OFFSET ?6"
        }
        (Some(_), None, true) => {
            "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_page
        WHERE is_tombstone = 0 AND subject_node_id = ?1
          AND observed_start >= ?3 - ?7 AND observed_start <= ?4 AND observed_end >= ?3
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT ?5 OFFSET ?6"
        }
        (Some(_), None, false) => {
            "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_query
        WHERE is_tombstone = 0 AND subject_node_id = ?1
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT ?5 OFFSET ?6"
        }
        (None, Some(_), true) => {
            "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_keyset
        WHERE is_tombstone = 0 AND schema_id = ?2
          AND observed_start >= ?3 - ?7 AND observed_start <= ?4 AND observed_end >= ?3
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT ?5 OFFSET ?6"
        }
        (None, Some(_), false) => {
            "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_keyset
        WHERE is_tombstone = 0 AND schema_id = ?2
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT ?5 OFFSET ?6"
        }
        (None, None, true) => {
            "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_keyset
        WHERE is_tombstone = 0
          AND observed_start >= ?3 - ?7 AND observed_start <= ?4 AND observed_end >= ?3
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT ?5 OFFSET ?6"
        }
        (None, None, false) => {
            "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_keyset
        WHERE is_tombstone = 0
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT ?5 OFFSET ?6"
        }
    }
}
