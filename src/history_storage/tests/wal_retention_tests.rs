use super::*;

fn wal_bytes(directory: &std::path::Path) -> u64 {
    fs::metadata(directory.join("history.sqlite3-wal"))
        .map(|metadata| metadata.len())
        .unwrap_or_default()
}

#[test]
fn reused_wal_allocation_does_not_keep_repository_over_quota() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    storage
        .write(NODE_HISTORY_KEY, &vec![7; 8 * 1024 * 1024])
        .unwrap();
    assert!(wal_bytes(temporary.path()) > 1024 * 1024);
    assert!(
        storage
            .repository_history_used_bytes_with_caller("wal-retention-test")
            .unwrap()
            > 10 * 1024 * 1024
    );
    storage.write(NODE_HISTORY_KEY, b"small").unwrap();
    for index in 0..4 {
        storage
            .write(USAGE_KEY, format!("usage-{index}").as_bytes())
            .unwrap();
    }
    assert!(wal_bytes(temporary.path()) <= 1024 * 1024);
    assert!(
        storage
            .repository_history_used_bytes_with_caller("wal-retention-test")
            .unwrap()
            < 10 * 1024 * 1024
    );
    assert_eq!(
        storage.read(NODE_HISTORY_KEY).unwrap(),
        Some(b"small".to_vec())
    );
    drop(storage);
    let restarted = HistoryStorage::open(temporary.path());
    assert_eq!(
        restarted.read(NODE_HISTORY_KEY).unwrap(),
        Some(b"small".to_vec())
    );
}

#[test]
fn wal_reuse_limit_preserves_pinned_reader_and_committed_payloads() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(temporary.path());
    storage.write(NODE_HISTORY_KEY, b"before").unwrap();
    let reader = rusqlite::Connection::open_with_flags(
        temporary.path().join(SQLITE_FILE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    reader.execute_batch("BEGIN").unwrap();
    let old: Vec<u8> = reader
        .query_row(
            "SELECT payload FROM history_snapshots WHERE key = ?1",
            [NODE_HISTORY_KEY],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(old, b"before");
    storage
        .write(NODE_HISTORY_KEY, &vec![9; 8 * 1024 * 1024])
        .unwrap();
    assert!(wal_bytes(temporary.path()) > 1024 * 1024);
    assert_eq!(
        storage.read(NODE_HISTORY_KEY).unwrap().unwrap(),
        vec![9; 8 * 1024 * 1024]
    );
    let still_old: Vec<u8> = reader
        .query_row(
            "SELECT payload FROM history_snapshots WHERE key = ?1",
            [NODE_HISTORY_KEY],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(still_old, b"before");
    reader.execute_batch("COMMIT").unwrap();
    drop(reader);
    // Identical UPSERTs need not dirty a page and cannot trigger a WAL reset.
    for index in 0..4 {
        storage
            .write(USAGE_KEY, format!("usage-{index}").as_bytes())
            .unwrap();
    }
    assert!(wal_bytes(temporary.path()) <= 1024 * 1024);
    drop(storage);
    let restarted = HistoryStorage::open(temporary.path());
    assert_eq!(
        restarted.read(NODE_HISTORY_KEY).unwrap().unwrap(),
        vec![9; 8 * 1024 * 1024]
    );
}
