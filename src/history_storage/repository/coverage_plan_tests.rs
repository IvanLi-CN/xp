//! Coverage query-plan assertions.
//!
//! Coverage is the panel's slowest read, so its plan is asserted directly rather than inferred from
//! timing: each boundary must reach its own index edge instead of scanning a subject's full slice.

use super::*;

#[test]
fn repository_coverage_query_uses_a_subject_seek_without_table_reads() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    // Subject indexes are maintenance work, not startup work: build them as the worker does.
    assert!(storage.build_repository_subject_indexes().unwrap());
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("test storage should use SQLite");
    };
    // Each boundary needs its own index edge; a combined aggregate scanned the whole subject.
    for (sql, index) in [
        (
            repository::COVERAGE_SUBJECT_OBSERVED_START_SQL,
            "subject_start",
        ),
        (repository::COVERAGE_SUBJECT_OBSERVED_END_SQL, "subject_end"),
        (
            repository::COVERAGE_SUBJECT_RECEIVED_SQL,
            "subject_received",
        ),
    ] {
        let plan = query_plan_with_params(connection, sql, ["subject-a"]);
        assert!(
            plan.iter()
                .any(|detail| detail.contains(index) && detail.contains("subject_node_id=?")),
            "expected {index} seek, got {plan:?}"
        );
    }
    let incomplete_plan = query_plan_with_params(
        connection,
        repository::INCOMPLETE_AGGREGATE_SUBJECT_SQL,
        rusqlite::params!["subject-a", "runtime.v1", 100_i64, 1_i64],
    );
    assert!(incomplete_plan.iter().any(|detail| {
        detail.contains("repository_history_records_aggregate_completeness")
            && detail.contains("subject_node_id=?")
    }));
}
