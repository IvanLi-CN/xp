use super::*;

#[test]
fn source_delivery_capacity_guard_preserves_cursor_and_backlog() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    runtime
        .queue_local_source_segment(
            "cluster-a",
            source_identity.clone(),
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:0".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            100,
        )
        .expect("queue initial source segment");
    drop(runtime);

    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    connection
        .execute(
            "UPDATE source_delivery_journal_state
             SET pending_segments = 20_000,
                 pending_bytes = 128 * 1024 * 1024,
                 capacity_suspended = 1,
                 order_repair_completed = 1
             WHERE singleton = 1",
            [],
        )
        .expect("saturate source delivery journal");
    drop(connection);

    let mut runtime = load(temporary.path());
    let result = runtime.queue_local_source_segment(
        "cluster-a",
        source_identity,
        &signing_key,
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "runtime.v1",
            1,
            b"runtime:1".to_vec(),
            b"sample".to_vec(),
            false,
        )],
        101,
    );
    assert!(
        result.is_err(),
        "source capture must pause while the durable journal is suspended"
    );
    assert_eq!(runtime.local_source_next_sequence("runtime"), Some(1));
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let summary = storage
        .source_delivery_journal_summary()
        .expect("read suspended journal summary");
    assert_eq!(summary.pending_segments, 20_000);
    assert_eq!(summary.pending_bytes, 128 * 1024 * 1024);
    assert!(summary.capacity_suspended);
    let status = runtime
        .source_delivery_status(101, false, 1024 * 1024 * 1024)
        .expect("read capacity guard status");
    assert_eq!(status.state, "journal_capacity_guard");
}

#[test]
fn source_delivery_capacity_guard_clears_only_below_both_low_watermarks() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let mut runtime = load(temporary.path());
    runtime
        .queue_local_source_segment(
            "cluster-a",
            identity(),
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:0".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            100,
        )
        .expect("queue source segment");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let id = storage.source_delivery_journal().unwrap()[0].id.clone();
    {
        let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
            .expect("open history database");
        connection
            .execute(
                "UPDATE source_delivery_journal_state
                 SET pending_segments = 100,
                     pending_bytes = 50 * 1024 * 1024,
                     capacity_suspended = 1
                 WHERE singleton = 1",
                [],
            )
            .expect("mark capacity suspension");
    }
    storage
        .acknowledge_source_delivery_journal(&[id], Some(101), Some("direct"))
        .expect("acknowledge below low watermarks");
    assert!(
        !storage
            .source_delivery_journal_summary()
            .expect("read recovered capacity state")
            .capacity_suspended
    );
}

#[test]
fn source_delivery_capacity_rejection_persists_suspension_without_writing() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let mut runtime = load(temporary.path());
    runtime
        .queue_local_source_segment(
            "cluster-a",
            identity(),
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:0".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            100,
        )
        .expect("queue source segment");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let row = storage
        .source_delivery_journal()
        .expect("read queued source journal")
        .into_iter()
        .next()
        .expect("read queued source segment");
    runtime
        .acknowledge_local_source_segment(&row.wire)
        .expect("clear source segment before capacity preflight");
    drop(runtime);

    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    connection
        .execute(
            "UPDATE source_delivery_journal_state
             SET pending_segments = 19_999,
                 pending_bytes = 128 * 1024 * 1024 - 1,
                 capacity_suspended = 0
             WHERE singleton = 1",
            [],
        )
        .expect("set near-capacity journal state");
    drop(connection);

    let error = storage
        .append_source_delivery_journal(std::slice::from_ref(&row))
        .expect_err("capacity preflight must reject the write");
    assert_eq!(error.to_string(), "source delivery journal capacity guard");
    let summary = storage
        .source_delivery_journal_summary()
        .expect("read rejected journal state");
    assert_eq!(summary.pending_segments, 19_999);
    assert_eq!(summary.pending_bytes, 128 * 1024 * 1024 - 1);
    assert!(summary.capacity_suspended);
    assert!(
        storage
            .source_delivery_journal()
            .expect("read journal after rejected write")
            .is_empty(),
        "rejected capacity preflight must not write or delete rows"
    );
}
