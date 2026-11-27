use super::*;

/// Whole-repository coverage from four independent index boundary seeks.
///
/// `MIN(observed_start)`, `MAX(observed_end)`, `MIN(received_at)` and `MAX(received_at)` each read
/// one boundary of an index whose prefix is `is_tombstone`, so none of them touches retained
/// payload or walks the table. A single four-column aggregate cannot do that: it must visit every
/// index entry, and its cost grows with the repository instead of staying constant.
/// Subject-scoped coverage from four independent boundary seeks.
///
/// Mirrors [`repository_history_coverage_boundaries`]: each column reads one index edge instead of
/// the whole matching slice. Ordering the checks so an empty subject returns early keeps an unused
/// node cheap.
pub(super) fn subject_coverage_boundaries(
    connection: &Connection,
    subject_node_id: &str,
) -> Result<Option<super::RepositoryHistoryCoverage>> {
    let observed_start = connection
        .query_row(
            super::COVERAGE_SUBJECT_OBSERVED_START_SQL,
            params![subject_node_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(sqlite_error)?;
    let Some(observed_start) = observed_start else {
        return Ok(None);
    };
    let observed_end = connection
        .query_row(
            super::COVERAGE_SUBJECT_OBSERVED_END_SQL,
            params![subject_node_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sqlite_error)?;
    let (received_start, received_end) = connection
        .query_row(
            super::COVERAGE_SUBJECT_RECEIVED_SQL,
            params![subject_node_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(sqlite_error)?;
    Ok(Some(super::RepositoryHistoryCoverage {
        observed_start_unix_seconds: u64::try_from(observed_start).unwrap_or(u64::MAX),
        observed_end_unix_seconds: u64::try_from(observed_end).unwrap_or(u64::MAX),
        received_start_unix_seconds: u64::try_from(received_start).unwrap_or(u64::MAX),
        received_end_unix_seconds: u64::try_from(received_end).unwrap_or(u64::MAX),
    }))
}

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
#[cfg(test)]
pub(crate) fn repository_history_records_sql(
    subject_node_id: Option<&str>,
    schema_id: Option<&str>,
    start_unix_seconds: Option<u64>,
) -> &'static str {
    repository_history_records_sql_with(subject_node_id, schema_id, start_unix_seconds, true)
}

/// Same statement, but able to omit the page-index hint.
///
/// The hint is what stops SQLite choosing an index that cannot evaluate the time filter. It is
/// also a hard requirement: a hinted statement fails when the named index is absent. While the
/// background worker is still building it, callers pass `page_index_ready = false` and get the
/// unhinted statement, which is slower but correct.
pub(crate) fn repository_history_records_sql_with(
    subject_node_id: Option<&str>,
    schema_id: Option<&str>,
    start_unix_seconds: Option<u64>,
    page_index_ready: bool,
) -> &'static str {
    // The superseded index still serves the windowless branches (`repository_history_records`),
    // which have no time filter and therefore no ?7.
    if subject_node_id.is_some() && !page_index_ready {
        return match (schema_id.is_some(), start_unix_seconds.is_some()) {
            (true, true) => {
                "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_query
        WHERE is_tombstone = 0 AND subject_node_id = ?1 AND schema_id = ?2
          AND observed_start >= ?3 - ?7 AND observed_start <= ?4 AND observed_end >= ?3
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence
        LIMIT ?5 OFFSET ?6"
            }
            (true, false) => {
                "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_query
        WHERE is_tombstone = 0 AND subject_node_id = ?1 AND schema_id = ?2
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence
        LIMIT ?5 OFFSET ?6"
            }
            (false, true) => {
                "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_query
        WHERE is_tombstone = 0 AND subject_node_id = ?1
          AND observed_start >= ?3 - ?7 AND observed_start <= ?4 AND observed_end >= ?3
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence
        LIMIT ?5 OFFSET ?6"
            }
            (false, false) => {
                "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_query
        WHERE is_tombstone = 0 AND subject_node_id = ?1
        ORDER BY observed_start, source_node_id, source_epoch, stream, sequence
        LIMIT ?5 OFFSET ?6"
            }
        };
    }
    match (subject_node_id, schema_id, start_unix_seconds.is_some()) {
        (Some(_), Some(_), true) => {
            "SELECT source_node_id, source_epoch, stream, sequence, subject_node_id,
               observer_node_id, schema_id, schema_version, record_key, is_tombstone,
               observed_start, observed_end, received_at, payload
        FROM repository_history_records INDEXED BY repository_history_records_page_v2
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
        FROM repository_history_records INDEXED BY repository_history_records_page_v2
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

/// Index that answers a subject-scoped record page without touching the table for rows the filter
/// will discard.
///
/// `is_tombstone` and `observed_end` must be in the index: the filter tests both, and neither can
/// be evaluated from `(subject_node_id, observed_start)` alone. Without them SQLite walks the
/// widened `observed_start` range and reads one payload page per candidate before rejecting it,
/// which turns a single page of a 24-hour window into tens of thousands of random reads. The five
/// ordering columns keep the published continuation order, and `schema_id` serves the
/// resource-monitoring branch.
pub(crate) const PAGE_INDEX_NAME: &str = "repository_history_records_page_v2";

/// Whether the record page index is already in place.
///
/// Startup must answer this cheaply: building the index on a multi-gigabyte repository takes over
/// a minute, and a container readiness probe that waits only twenty seconds would restart the
/// process before the build could commit. The build therefore belongs to a background worker, and
/// startup only checks for it.
pub(crate) fn page_index_present(connection: &Connection) -> Result<bool> {
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
            [PAGE_INDEX_NAME],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    Ok(count == 1)
}

/// Build the record page index and drop the superseded one.
///
/// This is deliberately not part of startup schema preparation. Queries stay correct without it;
/// they simply fall back to a table lookup per candidate row, so a slow build degrades latency
/// rather than availability.
pub(crate) fn build_page_index(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(concat!(
            "CREATE INDEX IF NOT EXISTS repository_history_records_page_v2
               ON repository_history_records
                  (subject_node_id, is_tombstone, observed_start, source_node_id, source_epoch,
                   stream, sequence, observed_end, schema_id);",
            " DROP INDEX IF EXISTS repository_history_records_page;",
        ))
        .map_err(sqlite_error)
}
