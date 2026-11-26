use super::*;
#[test]
fn record_page_predicates_seek_instead_of_scanning_the_index() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    // The page index is built by a background worker, not by startup preparation.
    assert!(storage.build_repository_page_index().unwrap());
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("test storage should use SQLite");
    };
    let span = MAX_OBSERVED_SPAN_SECONDS;
    for (subject, schema) in [
        (Some("subject-a"), None),
        (Some("subject-a"), Some("runtime.v1")),
        (None, Some("runtime.v1")),
        (None, None),
    ] {
        let sql =
            repository::query_sql::repository_history_records_sql(subject, schema, Some(1000));
        let plan = query_plan_with_params(
            connection,
            sql,
            rusqlite::params![
                subject,
                schema,
                1000_i64,
                2000_i64,
                101_i64,
                0_i64,
                span as i64
            ],
        );
        // Every branch must constrain observed_start, not merely is_tombstone. Without that the
        // planner scans the whole index and a 5 GB repository is read to answer a one-day query.
        assert!(
            plan.iter()
                .any(|detail| detail.contains("observed_start>?")
                    && detail.contains("observed_start<?")),
            "missing bounded observed_start seek for subject={subject:?} \
             schema={schema:?}: {plan:?}"
        );
        assert!(
            !plan
                .iter()
                .any(|detail| detail.contains("SCAN repository_history_records")),
            "predicate not pushed down for subject={subject:?} schema={schema:?}: {plan:?}"
        );
        // A subject page must be answered by the index alone. When `observed_end` or
        // `is_tombstone` is missing from the index, every candidate row needs a table lookup
        // before the filter can discard it, and one page of a 24-hour window then reads tens of
        // thousands of payload pages. Sub-second bounds depend on this staying a covering scan.
        if subject.is_some() {
            // `payload` forces a table read for the rows that survive, which is unavoidable and
            // cheap for one page. What must never happen is reading the table for rows the filter
            // will discard: `observed_end` and `is_tombstone` therefore have to live in the index
            // so the seek range is decided without touching the table.
            let seek = plan
                .iter()
                .find(|detail| detail.contains("repository_history_records_page_v2"))
                .unwrap_or_else(|| panic!("subject page must use the page index: {plan:?}"));
            assert!(
                seek.contains("is_tombstone=?")
                    && seek.contains("subject_node_id=?")
                    && seek.contains("observed_start"),
                "page index must decide the whole filter range, got: {seek}"
            );
        }
        let _ = span;
    }
}

#[test]
fn whole_repository_coverage_seeks_each_boundary_without_scanning() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    // The page index is built by a background worker, not by startup preparation.
    assert!(storage.build_repository_page_index().unwrap());
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("test storage should use SQLite");
    };
    for (sql, index) in [
        (
            "SELECT MIN(observed_start) FROM repository_history_records
             INDEXED BY repository_history_records_keyset WHERE is_tombstone = 0",
            "repository_history_records_keyset",
        ),
        (
            "SELECT MAX(observed_end) FROM repository_history_records
             INDEXED BY repository_history_records_export_filter WHERE is_tombstone = 0",
            "repository_history_records_export_filter",
        ),
        (
            "SELECT MIN(received_at) FROM repository_history_records
             INDEXED BY repository_history_records_received WHERE is_tombstone = 0",
            "repository_history_records_received",
        ),
        (
            "SELECT MAX(received_at) FROM repository_history_records
             INDEXED BY repository_history_records_received WHERE is_tombstone = 0",
            "repository_history_records_received",
        ),
    ] {
        let plan = query_plan(connection, sql);
        assert!(
            plan.iter().any(|detail| detail.contains(index)),
            "coverage boundary did not use {index}: {plan:?}"
        );
        assert!(
            !plan.iter().any(|detail| detail.contains("SCAN")),
            "coverage boundary scanned instead of seeking: {plan:?}"
        );
    }
}

#[test]
fn page_and_received_indexes_exist_for_existing_databases() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    // The page index is built by a background worker, not by startup preparation.
    assert!(storage.build_repository_page_index().unwrap());
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("test storage should use SQLite");
    };
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master
              WHERE type = 'index' AND tbl_name = 'repository_history_records'",
        )
        .unwrap();
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    for expected in [
        "repository_history_records_page_v2",
        "repository_history_records_received",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected} in {names:?}"
        );
    }
}
