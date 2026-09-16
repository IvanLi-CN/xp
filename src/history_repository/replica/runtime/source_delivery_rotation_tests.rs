use super::*;
use sha2::Sha256;

#[test]
fn source_delivery_replay_page_rotates_before_exhausting_a_hot_stream() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    for sequence in 0..256_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "connections.v1",
                    1,
                    format!("connection:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue hot stream backlog");
    }
    runtime
        .queue_local_source_segment(
            "cluster-a",
            source_identity,
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
            256,
        )
        .expect("queue later stream backlog");

    let page = runtime.local_source_pending_segments_page();
    let streams = page
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("signed replay segment")
                .canonical()
                .first_cursor()
                .stream()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        streams.contains(&"runtime".to_owned()),
        "a later stream head must not be starved by the hot stream"
    );
}

#[test]
fn source_delivery_capture_returns_the_rotated_page_to_the_worker() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = super::load(temporary.path());

    for sequence in 0..256_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "connections.v1",
                    1,
                    format!("connection:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue hot stream backlog");
    }

    let returned = runtime
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            source_identity,
            &signing_key,
            [SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:0".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            256,
            &["local".to_owned()],
        )
        .expect("capture later stream");
    assert!(returned.iter().any(|segment| {
        SignedSegment::from_wire(&segment.wire)
            .expect("signed returned segment")
            .canonical()
            .first_cursor()
            .stream()
            == "runtime"
    }));
}

#[test]
fn source_delivery_appends_after_unloaded_tail_without_overtaking_it() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = super::identity();
    let mut runtime = super::load(temporary.path());

    for sequence in 0..257_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "connections.v1",
                    1,
                    format!("connection:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue connections backlog");
    }
    drop(runtime);

    let mut restarted = super::load(temporary.path());
    assert!(
        !restarted
            .source_delivery_capture_paused()
            .expect("read source capture guard")
    );
    restarted
        .queue_local_source_segment(
            "cluster-a",
            source_identity,
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "connections.v1",
                1,
                b"connection:257".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            257,
        )
        .expect("capture after durable tail");
    assert_eq!(
        restarted.local_source_next_sequence("connections"),
        Some(258)
    );
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    assert!(
        storage
            .source_delivery_journal()
            .expect("read durable source journal")
            .iter()
            .any(|row| {
                SignedSegment::from_wire(&row.wire)
                    .expect("decode durable source segment")
                    .canonical()
                    .first_cursor()
                    .sequence()
                    == 257
            })
    );
    let page = restarted.local_source_pending_segments_page();
    assert!(page.iter().all(|segment| {
        SignedSegment::from_wire(&segment.wire)
            .expect("signed replay segment")
            .canonical()
            .first_cursor()
            .sequence()
            < 257
    }));
}

#[test]
fn source_delivery_partial_ack_keeps_window_stable_with_durable_tail() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = super::identity();
    super::source_delivery_capacity_tests::append_signed_source_journal_rows(
        temporary.path(),
        &signing_key,
        &source_identity,
        "runtime",
        "runtime.v1",
        0..257,
    );
    super::source_delivery_capacity_tests::append_signed_source_journal_rows(
        temporary.path(),
        &signing_key,
        &source_identity,
        "traffic",
        "traffic.v1",
        0..2,
    );

    let mut runtime = super::load(temporary.path());
    let initial_page = runtime.local_source_pending_segments_page();
    assert_eq!(initial_page.len(), 255);
    let first = initial_page
        .first()
        .expect("initial replay segment")
        .clone();
    runtime
        .acknowledge_local_source_segments_via_without_hydrating(
            std::slice::from_ref(&first),
            200,
            "direct",
        )
        .expect("acknowledge one replay segment");
    let remaining_ids = runtime
        .local_source_pending_segments_page()
        .iter()
        .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
        .collect::<Vec<_>>();

    runtime
        .hydrate_source_delivery_journal()
        .expect("hydrate the next worker cycle");
    let next_ids = runtime
        .local_source_pending_segments_page()
        .iter()
        .map(|segment| hex::encode(Sha256::digest(&segment.wire)))
        .collect::<Vec<_>>();
    assert_eq!(
        next_ids, remaining_ids,
        "a partial ACK must not rotate an unacknowledged window past its durable tail"
    );
}

#[test]
fn deferred_source_journal_ack_failure_does_not_advance_replay_cursor() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = super::load(temporary.path());
    for (stream, schema, sequence) in [
        ("connections", "connections.v1", 0_u64),
        ("runtime", "runtime.v1", 0_u64),
    ] {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    schema,
                    1,
                    format!("{stream}:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let page = runtime.local_source_pending_segments_page();
    let cursor_before = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open source journal database")
        .query_row(
            "SELECT replay_stream_cursor
             FROM source_delivery_journal_state
             WHERE singleton = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("read replay cursor before failed ACK");
    storage
        .set_query_only_for_test(true)
        .expect("enable SQLite write failure");

    assert!(
        runtime
            .acknowledge_local_source_segments_via_without_hydrating(&page[..1], 200, "direct",)
            .is_err()
    );
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("reopen source journal database");
    assert_eq!(
        connection
            .query_row(
                "SELECT replay_stream_cursor
                 FROM source_delivery_journal_state
                 WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .expect("read replay cursor after failed ACK"),
        cursor_before,
        "a failed deferred ACK must retain the durable replay cursor"
    );
    assert_eq!(
        storage
            .source_delivery_journal_summary()
            .expect("read journal after failed ACK")
            .pending_segments,
        2,
        "a failed deferred ACK must retain the durable journal"
    );
    assert_eq!(runtime.local_source_replay_window_cursor(), None);
}
