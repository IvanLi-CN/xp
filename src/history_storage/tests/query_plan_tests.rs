use super::*;
#[test]
fn record_page_predicates_seek_instead_of_scanning_the_index() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
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
        let _ = span;
    }
}

#[test]
fn whole_repository_coverage_seeks_each_boundary_without_scanning() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
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
        "repository_history_records_page",
        "repository_history_records_received",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected} in {names:?}"
        );
    }
}
