use super::*;

#[test]
fn source_delivery_journal_order_repair_is_resumable() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    for sequence in 0..257_u64 {
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
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }
    drop(runtime);

    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    connection
        .execute(
            "UPDATE source_delivery_journal
             SET source_node_id = '', source_epoch = 0, first_sequence = 0",
            [],
        )
        .expect("simulate legacy journal order columns");
    connection
        .execute(
            "UPDATE source_delivery_journal_state
             SET order_repair_cursor_id = NULL, order_repair_completed = 0
             WHERE singleton = 1",
            [],
        )
        .expect("mark journal order repair pending");
    drop(connection);

    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize journal while repairing");
    assert!(summary.order_repairing);
    assert!(summary.oldest.is_none());
    assert!(matches!(
        storage
            .source_delivery_journal_page(256)
            .expect("read journal page while repairing"),
        crate::state::history_storage::SourceDeliveryJournalPage::Repairing
    ));
    let runtime = load(temporary.path());
    let status = runtime
        .source_delivery_status(100, false, 1024 * 1024 * 1024)
        .expect("read source delivery status while repairing");
    assert_eq!(status.state, "journal_order_repairing");
    assert_eq!(status.pending_segments, 257);
    drop(runtime);

    let first = storage
        .repair_source_delivery_journal_order_page()
        .expect("repair first journal page");
    assert_eq!(first.processed, 256);
    assert!(!first.completed);
    let remaining = {
        let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
            .expect("reopen history database");
        connection
            .query_row(
                "SELECT COUNT(*) FROM source_delivery_journal WHERE source_node_id = ''",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count unrepaired journal rows")
    };
    assert_eq!(remaining, 1);

    drop(storage);
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let second = storage
        .repair_source_delivery_journal_order_page()
        .expect("resume journal repair");
    assert_eq!(second.processed, 1);
    assert!(second.completed);
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize repaired journal");
    assert!(!summary.order_repairing);
    let repaired_count = {
        let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
            .expect("reopen repaired history database");
        connection
            .query_row("SELECT COUNT(*) FROM source_delivery_journal", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count repaired journal rows")
    };
    assert_eq!(repaired_count, 257);
}

#[test]
fn source_delivery_order_repair_persists_new_segments_without_publishing_them() {
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
                b"legacy".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            1,
        )
        .expect("queue legacy source segment");
    drop(runtime);

    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    connection
        .execute(
            "UPDATE source_delivery_journal
             SET source_node_id = '', source_epoch = 0, first_sequence = 0",
            [],
        )
        .expect("simulate legacy journal order columns");
    connection
        .execute(
            "UPDATE source_delivery_journal_state
             SET order_repair_cursor_id = NULL, order_repair_completed = 0
             WHERE singleton = 1",
            [],
        )
        .expect("mark journal order repair pending");
    drop(connection);

    let mut runtime = load(temporary.path());
    let published = runtime
        .queue_local_source_segment(
            "cluster-a",
            source_identity,
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"new".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            2,
        )
        .expect("persist source segment during repair");
    assert!(
        published.is_none(),
        "repairing journal must not publish a page"
    );
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    assert_eq!(
        storage
            .source_delivery_journal_summary()
            .unwrap()
            .pending_segments,
        2
    );

    runtime
        .repair_source_delivery_journal_order_page()
        .expect("finish source journal repair");
    runtime
        .hydrate_source_delivery_journal()
        .expect("hydrate repaired journal");
    // The runtime exposes one front per stream; both durable rows remain available for drain.
    assert_eq!(runtime.local_source_pending_segments().len(), 1);
}
