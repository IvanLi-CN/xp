use super::*;

#[test]
fn repository_segment_schema_drops_the_redundant_descending_sync_index() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("test storage should use SQLite");
    };

    let index_names = connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND name LIKE 'repository_history_segments_sync_order%'",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(
        index_names,
        vec!["repository_history_segments_sync_order_v2".to_owned()]
    );
}
