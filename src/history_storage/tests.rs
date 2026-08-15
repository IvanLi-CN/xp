use std::{fs, time::Duration};

use super::*;

#[test]
fn migrates_legacy_snapshots_into_sqlite_without_rewriting_json() {
    let temporary = tempfile::tempdir().unwrap();
    let snapshots = [
        ("state.json", STATE_KEY, b"state".as_slice()),
        ("usage.json", USAGE_KEY, b"usage".as_slice()),
        (
            "inbound_ip_usage.json",
            INBOUND_IP_USAGE_KEY,
            b"inbound-ip".as_slice(),
        ),
        (
            "tcp_connection_usage.json",
            TCP_CONNECTION_USAGE_KEY,
            b"tcp-connections".as_slice(),
        ),
        (
            "node_history_cache.json",
            NODE_HISTORY_KEY,
            b"node-history".as_slice(),
        ),
        (
            "mesh/telemetry.json",
            MESH_TELEMETRY_KEY,
            b"mesh-telemetry".as_slice(),
        ),
        (
            "history/repository_replica.json",
            REPOSITORY_REPLICA_KEY,
            b"repository-replica".as_slice(),
        ),
    ];
    for (relative_path, _, payload) in snapshots {
        let path = temporary.path().join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, payload).unwrap();
    }

    let storage = HistoryStorage::open(temporary.path());

    assert!(storage.is_sqlite());
    for (relative_path, key, payload) in snapshots {
        assert_eq!(storage.read(key).unwrap(), Some(payload.to_vec()));
        assert_eq!(
            fs::read(temporary.path().join(relative_path)).unwrap(),
            payload
        );
    }
}

#[test]
fn failed_migration_keeps_json_as_the_only_active_store() {
    let temporary = tempfile::tempdir().unwrap();
    fs::create_dir(temporary.path().join("history.sqlite3")).unwrap();
    let legacy_path = temporary.path().join("state.json");
    fs::write(&legacy_path, b"old").unwrap();

    let storage = HistoryStorage::open(temporary.path());
    storage.write(STATE_KEY, b"new").unwrap();

    assert!(!storage.is_sqlite());
    assert_eq!(storage.mode(), HistoryStorageMode::DegradedJson);
    assert_eq!(fs::read(&legacy_path).unwrap(), b"new");
}

#[test]
fn failed_sqlite_write_restores_json_then_continues_with_json_only() {
    let temporary = tempfile::tempdir().unwrap();
    let legacy_path = temporary.path().join("state.json");
    fs::write(&legacy_path, b"before").unwrap();
    let storage = HistoryStorage::open(temporary.path());
    set_query_only(&storage);

    storage.write(STATE_KEY, b"after").unwrap();

    assert!(!storage.is_sqlite());
    assert_eq!(fs::read(&legacy_path).unwrap(), b"after");
    storage.write(STATE_KEY, b"final").unwrap();
    assert_eq!(fs::read(&legacy_path).unwrap(), b"final");
}

#[test]
fn sqlite_write_failure_keeps_json_as_the_only_store_after_restart() {
    let temporary = tempfile::tempdir().unwrap();
    let legacy_path = temporary.path().join("state.json");
    fs::write(&legacy_path, b"before").unwrap();
    let storage = HistoryStorage::open(temporary.path());
    set_query_only(&storage);
    storage.write(STATE_KEY, b"after").unwrap();
    drop(storage);

    let restarted = HistoryStorage::open(temporary.path());

    assert!(!restarted.is_sqlite());
    assert_eq!(restarted.read(STATE_KEY).unwrap(), Some(b"after".to_vec()));
}

#[test]
fn handles_for_one_data_dir_share_the_json_fallback() {
    let temporary = tempfile::tempdir().unwrap();
    let first = HistoryStorage::open(temporary.path());
    let second = HistoryStorage::open(temporary.path());
    set_query_only(&first);

    first.write(STATE_KEY, b"fallback").unwrap();

    assert!(!second.is_sqlite());
    assert_eq!(second.read(STATE_KEY).unwrap(), Some(b"fallback".to_vec()));
}

#[test]
fn external_repository_history_prevents_lossy_json_fallback() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    let snapshot = br#"{"external_history":true}"#;
    storage.write(REPOSITORY_REPLICA_KEY, snapshot).unwrap();
    set_query_only(&storage);

    assert!(storage.write(STATE_KEY, b"cannot-write").is_err());
    assert!(storage.is_sqlite());
    assert_eq!(
        storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        Some(snapshot.to_vec())
    );
}

#[test]
fn restart_uses_the_committed_migration_instead_of_reimporting_json() {
    let temporary = tempfile::tempdir().unwrap();
    let legacy_path = temporary.path().join("state.json");
    fs::write(&legacy_path, b"first").unwrap();

    let storage = HistoryStorage::open(temporary.path());
    assert_eq!(storage.read(STATE_KEY).unwrap(), Some(b"first".to_vec()));
    drop(storage);
    fs::write(&legacy_path, b"newer-json").unwrap();

    let restarted = HistoryStorage::open(temporary.path());
    assert_eq!(restarted.read(STATE_KEY).unwrap(), Some(b"first".to_vec()));
}

#[test]
fn sqlite_write_does_not_double_write_the_json_backup() {
    let temporary = tempfile::tempdir().unwrap();
    let legacy_path = temporary.path().join("state.json");
    fs::write(&legacy_path, b"before").unwrap();
    let storage = HistoryStorage::open(temporary.path());

    storage.write(STATE_KEY, b"after").unwrap();

    assert_eq!(storage.read(STATE_KEY).unwrap(), Some(b"after".to_vec()));
    assert_eq!(fs::read(&legacy_path).unwrap(), b"before");
}

#[test]
fn removes_migrated_json_backups_after_thirty_days() {
    let temporary = tempfile::tempdir().unwrap();
    let legacy_path = temporary.path().join("state.json");
    fs::write(&legacy_path, b"before").unwrap();
    let storage = HistoryStorage::open(temporary.path());

    storage.cleanup_expired_backups_at(
        std::time::SystemTime::now() + Duration::from_secs(31 * 24 * 60 * 60),
    );

    assert!(!legacy_path.exists());
}

#[test]
fn configures_wal_bounded_checkpoint_and_incremental_vacuum() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    storage
        .write(NODE_HISTORY_KEY, &vec![7; 256 * 1024])
        .unwrap();
    let pages_before_shrink = sqlite_pragma(&storage, "page_count");
    storage.write(NODE_HISTORY_KEY, b"small").unwrap();
    storage.write(MESH_TELEMETRY_KEY, b"mesh").unwrap();
    storage.write(INBOUND_IP_USAGE_KEY, b"ip").unwrap();

    let journal_mode = sqlite_text_pragma(&storage, "journal_mode");
    let auto_vacuum = sqlite_pragma(&storage, "auto_vacuum");
    let auto_checkpoint = sqlite_pragma(&storage, "wal_autocheckpoint");
    let pages_after_shrink = sqlite_pragma(&storage, "page_count");
    let free_pages = sqlite_pragma(&storage, "freelist_count");

    assert_eq!(journal_mode, "wal");
    assert_eq!(auto_vacuum, 2);
    assert_eq!(auto_checkpoint, 64);
    assert!(pages_after_shrink <= pages_before_shrink);
    assert!(free_pages < 64);
}

#[test]
fn repository_keyset_indexes_avoid_a_full_sort_for_compaction_and_export() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("test storage should use SQLite");
    };
    let compaction_plan = query_plan(
        connection,
        "SELECT source_node_id FROM repository_history_records
             WHERE is_tombstone = 0 AND observed_start < 1000
             ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT 1",
    );
    assert!(
        compaction_plan
            .iter()
            .any(|detail| detail.contains("repository_history_records_keyset"))
    );
    assert!(
        !compaction_plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE"))
    );

    let compaction_continuation_plan = query_plan(
        connection,
        "SELECT source_node_id FROM repository_history_records
             INDEXED BY repository_history_records_keyset
             WHERE is_tombstone = 0 AND observed_start < 1000
               AND (observed_start, source_node_id, source_epoch, stream, sequence)
                   > (100, 'node-a', 7, 'traffic', 10)
             ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT 1",
    );
    assert!(
        compaction_continuation_plan
            .iter()
            .any(|detail| detail.contains("repository_history_records_keyset"))
    );
    assert!(compaction_continuation_plan.iter().any(|detail| {
        detail.contains("(observed_start,source_node_id,source_epoch,stream,sequence)>(?,?,?,?,?)")
    }));
    assert!(
        !compaction_continuation_plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE"))
    );

    let export_plan = query_plan(
        connection,
        "SELECT source_node_id FROM repository_history_records
             INDEXED BY repository_history_records_keyset
             WHERE is_tombstone = 0 AND observed_end < 1000 AND received_at <= 1000
               AND (observed_start, source_node_id, source_epoch, stream, sequence)
                   > (100, 'node-a', 7, 'traffic', 10)
               AND (observed_start, source_node_id, source_epoch, stream, sequence)
                   <= (900, 'node-z', 9, 'traffic', 999)
             ORDER BY observed_start, source_node_id, source_epoch, stream, sequence LIMIT 1",
    );
    assert!(export_plan.iter().any(|detail| {
        detail.contains("repository_history_records_keyset")
            || detail.contains("repository_history_records_export_filter")
    }));
    assert!(
        !export_plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE"))
    );
}

#[test]
fn repository_history_export_leases_are_expired_and_bounded() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    for index in 0..MAX_ACTIVE_REPOSITORY_HISTORY_EXPORTS {
        storage
            .refresh_repository_history_export(&format!("session-{index}"), 100)
            .unwrap();
    }
    assert!(
        storage
            .refresh_repository_history_export("one-too-many", 100)
            .is_err()
    );
    assert!(storage.has_active_repository_history_export(100).unwrap());
    assert!(
        !storage
            .has_active_repository_history_export(100 + REPOSITORY_HISTORY_EXPORT_LEASE_SECONDS)
            .unwrap()
    );
    storage
        .refresh_repository_history_export(
            "after-expiry",
            101 + REPOSITORY_HISTORY_EXPORT_LEASE_SECONDS,
        )
        .unwrap();
}

fn sqlite_pragma(storage: &HistoryStorage, pragma: &str) -> i64 {
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("test storage should use SQLite");
    };
    connection
        .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
        .unwrap()
}

fn sqlite_text_pragma(storage: &HistoryStorage, pragma: &str) -> String {
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("test storage should use SQLite");
    };
    connection
        .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
        .unwrap()
}

fn query_plan(connection: &rusqlite::Connection, query: &str) -> Vec<String> {
    connection
        .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
        .unwrap()
        .query_map([], |row| row.get(3))
        .unwrap()
        .collect::<std::result::Result<Vec<String>, _>>()
        .unwrap()
}

fn set_query_only(storage: &HistoryStorage) {
    let backend = storage.lock_backend();
    let Backend::Sqlite(connection) = &*backend else {
        panic!("test storage should use SQLite");
    };
    connection.pragma_update(None, "query_only", "ON").unwrap();
}
