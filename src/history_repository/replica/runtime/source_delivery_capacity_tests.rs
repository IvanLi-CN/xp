use super::*;
use sha2::Sha256;

pub(crate) fn append_signed_source_journal_rows(
    path: &std::path::Path,
    key: &SigningKey,
    source_identity: &crate::state::history_repository::identity::RepositoryNodeIdentity,
    stream: &str,
    schema: &str,
    sequences: std::ops::Range<u64>,
) {
    let rows = sequences
        .map(|sequence| {
            let wire = CanonicalSegment::new(
                "cluster-a",
                Cursor::new("node-a", 1, stream, sequence).expect("cursor"),
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    schema,
                    1,
                    format!("{stream}:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                None,
                sequence,
                sequence,
            )
            .expect("segment")
            .sign(key)
            .expect("sign segment")
            .wire_bytes()
            .expect("encode segment");
            crate::state::history_storage::SourceDeliveryJournalRow {
                id: hex::encode(Sha256::digest(&wire)),
                stream: stream.to_owned(),
                closed_at_unix_seconds: sequence,
                identity: source_identity.clone(),
                wire,
            }
        })
        .collect::<Vec<_>>();
    crate::state::history_repository::HistoryStorage::open(path)
        .append_source_delivery_journal(&rows)
        .expect("append signed source journal rows");
}

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

#[test]
fn source_delivery_capacity_guard_allows_replaying_existing_backlog() {
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

    let replay = runtime
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            source_identity,
            &signing_key,
            Vec::new(),
            101,
            &["repository-a".to_owned()],
        )
        .expect("capacity guard must not block replay");
    assert_eq!(replay.len(), 1);
    assert_eq!(runtime.local_source_next_sequence("runtime"), Some(1));
}

#[test]
fn source_delivery_replay_page_is_bounded() {
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
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                100 + sequence,
            )
            .expect("queue source backlog");
    }

    let page = runtime.local_source_pending_segments_page();
    assert!(page.len() <= 256);
    assert!(page.iter().map(|segment| segment.wire.len()).sum::<usize>() <= 1024 * 1024);
}

#[test]
fn source_delivery_hydration_keeps_each_stream_head_visible() {
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
            .expect("queue connections backlog");
    }
    runtime
        .queue_local_source_segment(
            "cluster-a",
            source_identity.clone(),
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "resource_metrics.v1",
                1,
                b"resource:0".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            300,
        )
        .expect("queue resource backlog");
    drop(runtime);

    let restored = load(temporary.path());
    let heads = restored.local_source_pending_segments();
    let head_cursors = heads
        .iter()
        .map(|segment| {
            let signed = SignedSegment::from_wire(&segment.wire).expect("signed head");
            let cursor = signed.canonical().first_cursor();
            (cursor.stream().to_owned(), cursor.sequence())
        })
        .collect::<Vec<_>>();

    assert!(head_cursors.contains(&("connections".to_owned(), 0)));
    assert!(head_cursors.contains(&("resource_metrics-v1".to_owned(), 0)));
}

#[test]
fn source_delivery_appends_after_unloaded_tail_without_overtaking_it() {
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

    let mut restarted = load(temporary.path());
    assert!(
        !restarted
            .source_delivery_capture_paused()
            .expect("read source capture guard")
    );
    let result = restarted.queue_local_source_segment(
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
    );
    result.expect("capture after durable tail");
    assert_eq!(
        restarted.local_source_next_sequence("connections"),
        Some(258)
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
fn source_delivery_hydration_shares_the_wire_budget_with_stream_heads() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let source_identity = identity();
    let identity_wire = serde_json::to_vec(&source_identity).expect("serialize source identity");
    let wire = vec![7_u8; 200 * 1024];
    let streams = [
        "runtime",
        "path_health",
        "traffic",
        "connections",
        "ip_usage",
        "uptime",
        "resource_metrics-v1",
        "tombstone",
    ];
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    let transaction = connection
        .unchecked_transaction()
        .expect("begin source journal transaction");
    for (sequence, stream) in streams.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO source_delivery_journal
                     (id, stream, closed_at, identity, wire, created_at,
                      source_node_id, source_epoch, first_sequence)
                 VALUES (?1, ?2, 100, ?3, ?4, 100, 'node-a', 1, ?5)",
                rusqlite::params![
                    format!("head-{stream}"),
                    stream,
                    &identity_wire,
                    &wire,
                    sequence as i64,
                ],
            )
            .expect("insert oversized stream head");
    }
    transaction
        .execute(
            "UPDATE source_delivery_journal_state
             SET pending_segments = 8,
                 pending_bytes = ?1,
                 epoch_high_water = 1,
                 order_repair_completed = 1
             WHERE singleton = 1",
            [wire.len() as i64 * streams.len() as i64],
        )
        .expect("record source journal statistics");
    transaction
        .commit()
        .expect("commit source journal transaction");
    drop(connection);
    drop(storage);

    let restored = load(temporary.path());
    let page = restored.local_source_pending_segments_page();
    assert_eq!(page.len(), 5, "heads must share the aggregate 1 MiB budget");
    assert!(page.len() <= 256);
    assert!(
        page.iter().map(|segment| segment.wire.len()).sum::<usize>()
            <= crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES
    );
    let live_capture_heads = restored.local_source_pending_segments();
    assert_eq!(
        live_capture_heads.len(),
        5,
        "live capture fronts must share the aggregate 1 MiB budget"
    );
    assert!(
        live_capture_heads
            .iter()
            .map(|segment| segment.wire.len())
            .sum::<usize>()
            <= crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES
    );
}

#[test]
fn source_delivery_stream_heads_rotate_after_budget_exhaustion() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let source_identity = identity();
    let identity_wire = serde_json::to_vec(&source_identity).expect("serialize source identity");
    let wire = vec![7_u8; 200 * 1024];
    let streams = [
        "runtime",
        "path_health",
        "traffic",
        "connections",
        "ip_usage",
        "uptime",
        "resource_metrics-v1",
        "tombstone",
    ];
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    let transaction = connection
        .unchecked_transaction()
        .expect("begin source journal transaction");
    for (sequence, stream) in streams.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO source_delivery_journal
                     (id, stream, closed_at, identity, wire, created_at,
                      source_node_id, source_epoch, first_sequence)
                 VALUES (?1, ?2, 100, ?3, ?4, 100, 'node-a', 1, ?5)",
                rusqlite::params![
                    format!("rotate-{stream}"),
                    stream,
                    &identity_wire,
                    &wire,
                    sequence as i64,
                ],
            )
            .expect("insert source journal row");
    }
    transaction
        .execute(
            "UPDATE source_delivery_journal_state
             SET pending_segments = 8,
                 pending_bytes = ?1,
                 replay_stream_cursor = NULL,
                 order_repair_completed = 1
             WHERE singleton = 1",
            [wire.len() as i64 * streams.len() as i64],
        )
        .expect("record source journal statistics");
    transaction
        .commit()
        .expect("commit source journal transaction");
    drop(connection);

    let first = storage
        .source_delivery_journal_stream_heads(
            &streams
                .iter()
                .map(|stream| (*stream).to_owned())
                .collect::<Vec<_>>(),
            crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS,
            crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES,
        )
        .expect("read first rotated heads");
    assert_eq!(first.len(), 5);
    assert_eq!(first[0].stream, "tombstone");
    let cursor_before_commit = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("reopen source journal database")
        .query_row(
            "SELECT replay_stream_cursor
             FROM source_delivery_journal_state
             WHERE singleton = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("read replay cursor before commit");
    assert!(cursor_before_commit.is_none());
    let first_ids = first.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    storage
        .acknowledge_source_delivery_journal_with_cursor(
            &first_ids,
            None,
            None,
            first
                .iter()
                .rev()
                .find(|row| row.stream != "tombstone")
                .map(|row| row.stream.as_str()),
        )
        .expect("acknowledge first rotated heads");

    let second = storage
        .source_delivery_journal_stream_heads(
            &streams
                .iter()
                .map(|stream| (*stream).to_owned())
                .collect::<Vec<_>>(),
            crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS,
            crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES,
        )
        .expect("read second rotated heads");
    let second_streams = second
        .iter()
        .map(|row| row.stream.as_str())
        .collect::<Vec<_>>();
    assert_eq!(second_streams, ["runtime", "traffic", "uptime"]);
}

#[test]
fn source_delivery_replay_window_is_stable_across_restart_before_ack() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let source_identity = identity();
    let identity_wire = serde_json::to_vec(&source_identity).expect("serialize source identity");
    let wire_len = 200 * 1024;
    let streams = [
        "runtime",
        "path_health",
        "traffic",
        "connections",
        "ip_usage",
        "uptime",
        "resource_metrics-v1",
        "tombstone",
    ];
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open source journal database");
    let transaction = connection
        .unchecked_transaction()
        .expect("begin source journal transaction");
    for (sequence, stream) in streams.iter().enumerate() {
        let wire = vec![u8::try_from(sequence + 1).expect("wire marker"); wire_len];
        transaction
            .execute(
                "INSERT INTO source_delivery_journal
                     (id, stream, closed_at, identity, wire, created_at,
                      source_node_id, source_epoch, first_sequence)
                 VALUES (?1, ?2, 100, ?3, ?4, 100, 'node-a', 1, ?5)",
                rusqlite::params![
                    hex::encode(Sha256::digest(&wire)),
                    stream,
                    &identity_wire,
                    &wire,
                    sequence as i64,
                ],
            )
            .expect("insert source journal row");
    }
    transaction
        .execute(
            "UPDATE source_delivery_journal_state
             SET pending_segments = 8,
                 pending_bytes = ?1,
                 epoch_high_water = 1,
                 replay_stream_cursor = NULL,
                 order_repair_completed = 1
             WHERE singleton = 1",
            [wire_len as i64 * streams.len() as i64],
        )
        .expect("record source journal statistics");
    transaction
        .commit()
        .expect("commit source journal transaction");
    drop(connection);
    drop(storage);

    let first = load(temporary.path());
    let first_page = first.local_source_pending_segments_page();
    let first_digests = first_page
        .iter()
        .map(|segment| Sha256::digest(&segment.wire))
        .collect::<Vec<_>>();
    drop(first);

    let mut restarted = load(temporary.path());
    let restarted_page = restarted.local_source_pending_segments_page();
    let restarted_digests = restarted_page
        .iter()
        .map(|segment| Sha256::digest(&segment.wire))
        .collect::<Vec<_>>();
    assert_eq!(restarted_digests, first_digests);
    let cursor_before_ack = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("reopen source journal database")
        .query_row(
            "SELECT replay_stream_cursor
             FROM source_delivery_journal_state
             WHERE singleton = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("read replay cursor before ack");
    assert!(cursor_before_ack.is_none());

    restarted
        .acknowledge_local_source_segments_via(&restarted_page, 200, "direct")
        .expect("acknowledge replay window");
    let cursor_after_ack = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("reopen source journal database")
        .query_row(
            "SELECT replay_stream_cursor
             FROM source_delivery_journal_state
             WHERE singleton = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("read replay cursor after ack");
    assert_eq!(cursor_after_ack.as_deref(), Some("resource_metrics-v1"));
}

#[test]
fn source_delivery_replay_cursor_persists_before_stream_tails_drain() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let source_identity = identity();
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let identity_wire = serde_json::to_vec(&source_identity).expect("serialize source identity");
    let payload_len = 175 * 1024;
    let streams = [
        "connections",
        "ip_usage",
        "path_health",
        "resource_metrics-v1",
        "runtime",
        "traffic",
        crate::uptime_monitor::UPTIME_HISTORY_STREAM,
    ];
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open source journal database");
    let transaction = connection
        .unchecked_transaction()
        .expect("begin source journal transaction");
    let mut pending_bytes = 0_i64;
    for (stream_index, stream) in streams.iter().enumerate() {
        for sequence in 0..2_i64 {
            let schema = match *stream {
                "connections" => "connections.v1",
                "ip_usage" => "ip_usage.v1",
                "path_health" => "path_health.v1",
                "resource_metrics-v1" => crate::resource_monitoring::RESOURCE_HISTORY_SCHEMA,
                "runtime" => "runtime.v1",
                "traffic" => "traffic.v1",
                crate::uptime_monitor::UPTIME_HISTORY_STREAM => {
                    crate::uptime_monitor::UPTIME_HISTORY_SCHEMA
                }
                _ => unreachable!("known source stream"),
            };
            let signed = CanonicalSegment::new(
                "cluster-a",
                Cursor::new("node-a", 1, *stream, sequence as u64).expect("source cursor"),
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    schema,
                    1,
                    format!("replay-{stream}-{sequence}").into_bytes(),
                    vec![u8::try_from(stream_index + 1).expect("wire marker"); payload_len],
                    false,
                )],
                None,
                100,
                100,
            )
            .expect("source segment")
            .sign(&signing_key)
            .expect("sign source segment");
            let wire = signed.wire_bytes().expect("encode source segment");
            pending_bytes += i64::try_from(wire.len()).expect("wire length");
            transaction
                .execute(
                    "INSERT INTO source_delivery_journal
                         (id, stream, closed_at, identity, wire, created_at,
                          source_node_id, source_epoch, first_sequence)
                     VALUES (?1, ?2, 100, ?3, ?4, 100, 'node-a', 1, ?5)",
                    rusqlite::params![
                        hex::encode(Sha256::digest(&wire)),
                        stream,
                        &identity_wire,
                        &wire,
                        sequence,
                    ],
                )
                .expect("insert source journal row");
        }
    }
    transaction
        .execute(
            "UPDATE source_delivery_journal_state
             SET pending_segments = ?1,
                 pending_bytes = ?2,
                 epoch_high_water = 1,
                 replay_stream_cursor = NULL,
                 order_repair_completed = 1
             WHERE singleton = 1",
            rusqlite::params![
                i64::try_from(streams.len() * 2).expect("segment count"),
                pending_bytes,
            ],
        )
        .expect("record source journal statistics");
    transaction
        .commit()
        .expect("commit source journal transaction");
    drop(connection);
    drop(storage);

    let mut runtime = load(temporary.path());
    let first_page = runtime.local_source_pending_segments();
    assert_eq!(first_page.len(), 5);
    let first_streams = first_page
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("decode first replay head")
                .canonical()
                .first_cursor()
                .stream()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        first_streams,
        [
            "connections",
            "ip_usage",
            "path_health",
            "resource_metrics-v1",
            "runtime"
        ]
    );

    runtime
        .acknowledge_local_source_segment_via(&first_page[0].wire, 200, "direct")
        .expect("acknowledge one replay head");
    let cursor_after_ack = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("reopen source journal database")
        .query_row(
            "SELECT replay_stream_cursor
             FROM source_delivery_journal_state
             WHERE singleton = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("read replay cursor after partial ack");
    assert_eq!(cursor_after_ack.as_deref(), Some("runtime"));
    assert_eq!(runtime.local_source_replay_window_cursor(), Some("runtime"));
    let second_page = runtime.local_source_pending_segments();
    runtime
        .acknowledge_local_source_segment_via(&second_page[0].wire, 201, "direct")
        .expect("acknowledge second replay head");
    assert_eq!(runtime.local_source_replay_window_cursor(), Some("runtime"));
    drop(runtime);

    let restarted = load(temporary.path());
    let next_streams = restarted
        .local_source_pending_segments()
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("decode rotated replay head")
                .canonical()
                .first_cursor()
                .stream()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        next_streams,
        [
            "ip_usage",
            "path_health",
            "resource_metrics-v1",
            "service_monitor_observation-v1",
            "traffic",
        ]
    );
}

#[test]
fn source_delivery_stream_heads_use_the_stream_index() {
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
            .expect("queue connections backlog");
    }
    drop(runtime);

    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let heads = storage
        .source_delivery_journal_stream_heads(
            &["connections".to_owned()],
            crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_SEGMENTS,
            crate::state::history_storage::SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES,
        )
        .expect("read requested stream head");
    assert_eq!(heads.len(), 1);

    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    let plan = connection
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT id, stream, closed_at, identity, wire
             FROM source_delivery_journal
             WHERE stream = 'connections'
             ORDER BY source_node_id, source_epoch, first_sequence, created_at, id
             LIMIT 1",
        )
        .expect("prepare stream-head plan")
        .query_map([], |row| row.get::<_, String>(3))
        .expect("read stream-head plan")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("collect stream-head plan")
        .join(" ");
    assert!(
        plan.contains("source_delivery_journal_cursor_order"),
        "stream head must use the bounded stream index: {plan}"
    );
}
