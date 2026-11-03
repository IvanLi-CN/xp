use super::*;
use crate::state::history_repository::identity::{
    Ed25519PublicKey, RepositoryNodeId, X25519PublicKey,
};

fn identity() -> Vec<u8> {
    serde_json::to_vec(
        &RepositoryNodeIdentity::new(
            RepositoryNodeId::try_from("source".to_owned()).unwrap(),
            Ed25519PublicKey::from_bytes([7; 32]).unwrap(),
            X25519PublicKey::from_bytes([8; 32]).unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
}

fn with_connection(storage: &HistoryStorage, action: impl FnOnce(&Connection)) {
    let mut backend = storage.lock_backend();
    action(sqlite_connection(&mut backend).unwrap().unwrap());
}

fn insert(storage: &HistoryStorage, id: &str, wire: &[u8], identity: &[u8]) {
    with_connection(storage, |connection| {
        connection
            .execute(
                "INSERT INTO source_delivery_journal
                (id, stream, closed_at, identity, wire, created_at)
             VALUES (?1, 'runtime', 42, ?2, ?3, 1)",
                params![id, identity, wire],
            )
            .unwrap();
    });
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn exact_lookup_preserves_order_and_legacy_rows_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    insert(&storage, "a", b"first", &identity());
    insert(&storage, "b", b"second", &identity());
    insert(&storage, "unselected", b"ignored", &identity());
    let mut changes_before = 0;
    with_connection(&storage, |connection| {
        connection
            .execute(
                "UPDATE source_delivery_journal_state SET order_repair_completed = 0,
             last_acknowledged_at = 123, last_delivery_path = 'public',
             replay_stream_cursor = 'runtime'",
                [],
            )
            .unwrap();
        changes_before = connection.total_changes();
        connection.pragma_update(None, "query_only", "ON").unwrap();
    });
    let rows = storage
        .source_delivery_journal_by_ids(&ids(&["b", "a"]), 11)
        .unwrap();
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert_eq!(rows[0].wire, b"second");
    assert_eq!(rows[1].wire, b"first");
    assert_eq!(rows[0].stream, "runtime");
    assert_eq!(rows[0].closed_at_unix_seconds, 42);
    assert_eq!(serde_json::to_vec(&rows[0].identity).unwrap(), identity());
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["a", "missing"]), 100)
            .is_err()
    );
    with_connection(&storage, |connection| {
        assert_eq!(connection.total_changes(), changes_before);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM source_delivery_journal WHERE source_node_id = ''",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            3
        );
        let state = connection
            .query_row(
                "SELECT order_repair_completed, last_acknowledged_at,
                    last_delivery_path, replay_stream_cursor
             FROM source_delivery_journal_state",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(state, (0, 123, "public".into(), "runtime".into()));
    });
}

#[test]
fn exact_lookup_rejects_duplicates_missing_ids_and_count_overflow() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let identity = identity();
    for index in 0..256 {
        insert(&storage, &index.to_string(), b"x", &identity);
    }
    let mut requested = (0..256).map(|index| index.to_string()).collect::<Vec<_>>();
    assert_eq!(
        storage
            .source_delivery_journal_by_ids(&requested, 256)
            .unwrap()
            .len(),
        256
    );
    requested.push("256".into());
    assert!(
        storage
            .source_delivery_journal_by_ids(&requested, usize::MAX)
            .unwrap_err()
            .0
            .contains("count")
    );
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["0", "0"]), 100)
            .is_err()
    );
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["0", "missing"]), 100)
            .is_err()
    );
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["missing"]), 100)
            .is_err()
    );
    assert!(
        storage
            .source_delivery_journal_by_ids(&["x".repeat(1025)], 100)
            .is_err()
    );
    assert!(
        storage
            .source_delivery_journal_by_ids(&[], 0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn exact_lookup_checks_entire_wire_budget_before_decoding_any_identity() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    // If the first payload were decoded before all checks, invalid JSON would win.
    insert(&storage, "a", b"123", b"invalid-json");
    insert(&storage, "b", b"456", &identity());
    let error = storage
        .source_delivery_journal_by_ids(&ids(&["a", "b"]), 5)
        .unwrap_err();
    assert!(error.0.contains("byte budget"), "{error:?}");
    let error = storage
        .source_delivery_journal_by_ids(&ids(&["a", "missing"]), 6)
        .unwrap_err();
    assert!(error.0.contains("missing"), "{error:?}");
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["b"]), 2)
            .is_err()
    );
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["b"]), 0)
            .is_err()
    );
    // A malformed payload fails the whole lookup after successful preflight.
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["b", "a"]), 6)
            .is_err()
    );
}

#[test]
fn exact_lookup_enforces_hard_wire_and_metadata_memory_caps() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let max = SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES;
    insert(&storage, "exact", &vec![1; max], &identity());
    assert_eq!(
        storage
            .source_delivery_journal_by_ids(&ids(&["exact"]), max)
            .unwrap()[0]
            .wire
            .len(),
        max
    );
    insert(&storage, "over", &vec![1; max + 1], &identity());
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["over"]), usize::MAX)
            .unwrap_err()
            .0
            .contains("byte budget")
    );
    insert(&storage, "metadata", b"", &vec![b' '; max + 1]);
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["metadata"]), max)
            .unwrap_err()
            .0
            .contains("byte budget")
    );
    insert(&storage, "stream", b"", &identity());
    with_connection(&storage, |connection| {
        connection
            .execute(
                "UPDATE source_delivery_journal SET stream = ?1 WHERE id = 'stream'",
                ["猫".repeat(max / 3 + 1)],
            )
            .unwrap();
    });
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["stream"]), max)
            .unwrap_err()
            .0
            .contains("byte budget")
    );
}

#[test]
fn exact_lookup_requires_sqlite_even_for_empty_requests() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join(SQLITE_FILE)).unwrap();
    let storage = HistoryStorage::open(directory.path());
    assert_eq!(storage.mode(), HistoryStorageMode::DegradedJson);
    assert!(storage.source_delivery_journal_by_ids(&[], 0).is_err());
}

#[test]
fn exact_lookup_bounds_aggregate_metadata_and_rejects_non_blob_wire() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let metadata = vec![b' '; SOURCE_DELIVERY_JOURNAL_PAGE_MAX_WIRE_BYTES / 2];
    insert(&storage, "a", b"", &metadata);
    insert(&storage, "b", b"", &metadata);
    let error = storage
        .source_delivery_journal_by_ids(&ids(&["a", "b"]), 0)
        .unwrap_err();
    assert!(error.0.contains("byte budget"), "{error:?}");

    insert(&storage, "text", b"", &identity());
    with_connection(&storage, |connection| {
        connection
            .execute(
                "UPDATE source_delivery_journal SET wire = '猫' WHERE id = 'text'",
                [],
            )
            .unwrap();
    });
    assert!(
        storage
            .source_delivery_journal_by_ids(&ids(&["text"]), 1)
            .is_err()
    );
}

fn successor_identity() -> (ed25519_dalek::SigningKey, RepositoryNodeIdentity) {
    let key = ed25519_dalek::SigningKey::from_bytes(&[11; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("source".to_owned()).unwrap(),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).unwrap(),
        X25519PublicKey::from_bytes([8; 32]).unwrap(),
    )
    .unwrap();
    (key, identity)
}

fn successor_segment(
    key: &ed25519_dalek::SigningKey,
    sequence: u64,
    count: usize,
    previous: Option<[u8; 32]>,
    epoch: u64,
    stream: &str,
) -> SignedSegment {
    use crate::history_sync::{CanonicalSegment, Cursor, SyncRecord};
    let records = (0..count)
        .map(|index| {
            SyncRecord::new(
                "subject",
                "observer",
                "runtime.v1",
                1,
                format!("{sequence}-{index}").into_bytes(),
                b"payload".to_vec(),
                false,
            )
        })
        .collect();
    CanonicalSegment::new(
        xp_test_fixtures::primary_cluster_id(),
        Cursor::new("source", epoch, stream, sequence).unwrap(),
        records,
        previous,
        10,
        42,
    )
    .unwrap()
    .sign(key)
    .unwrap()
}

fn insert_successor(
    storage: &HistoryStorage,
    identity: &RepositoryNodeIdentity,
    signed: &SignedSegment,
) {
    use sha2::{Digest as _, Sha256};
    let wire = signed.wire_bytes().unwrap();
    let id = hex::encode(Sha256::digest(&wire));
    let cursor = signed.canonical().first_cursor();
    with_connection(storage, |connection| {
        connection
            .execute(
                concat!(
                    "INSERT INTO source_delivery_journal ",
                    "(id,stream,closed_at,identity,wire,created_at,",
                    "source_node_id,source_epoch,first_sequence) ",
                    "VALUES (?1,?2,42,?3,?4,1,?5,?6,?7)"
                ),
                params![
                    id,
                    cursor.stream(),
                    serde_json::to_vec(identity).unwrap(),
                    wire,
                    cursor.source_node_id(),
                    cursor.source_epoch(),
                    cursor.sequence()
                ],
            )
            .unwrap();
    });
}

fn successor_page(
    storage: &HistoryStorage,
    identity: &RepositoryNodeIdentity,
    start: u64,
    end: u64,
    hash: [u8; 32],
    count: usize,
    bytes: usize,
) -> Result<(Vec<SourceDeliveryJournalRow>, u64, [u8; 32])> {
    storage.source_delivery_journal_successors(
        xp_test_fixtures::primary_cluster_id(),
        identity,
        7,
        "runtime",
        start,
        end,
        hash,
        count,
        bytes,
    )
}

#[test]
fn successor_pages_seek_index_and_resume_exact_chain_across_rounds() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 7, "runtime");
    let initial = genesis.segment_hash().unwrap();
    let mut previous = initial;
    for sequence in 1..=260 {
        let signed = successor_segment(&key, sequence, 1, Some(previous), 7, "runtime");
        previous = signed.segment_hash().unwrap();
        insert_successor(&storage, &identity, &signed);
    }
    with_connection(&storage, |connection| {
        let plan: String = connection
            .query_row(
                "EXPLAIN QUERY PLAN SELECT rowid FROM source_delivery_journal
             INDEXED BY source_delivery_journal_cursor_order
             WHERE stream='runtime' AND source_node_id='source' AND source_epoch=7
               AND first_sequence>=1 AND first_sequence<=260
             ORDER BY first_sequence,created_at,id LIMIT 257",
                [],
                |row| row.get(3),
            )
            .unwrap();
        assert!(
            plan.contains("source_delivery_journal_cursor_order"),
            "{plan}"
        );
        connection.pragma_update(None, "query_only", "ON").unwrap();
    });
    let (first, next, hash) =
        successor_page(&storage, &identity, 1, 261, initial, 256, 1024 * 1024).unwrap();
    assert_eq!(first.len(), 256);
    assert_eq!(next, 257);
    let (last, next, hash) =
        successor_page(&storage, &identity, next, 261, hash, 256, 1024 * 1024).unwrap();
    assert_eq!(last.len(), 4);
    assert_eq!(next, 261);
    assert_eq!(hash, previous);
    assert_eq!(
        successor_page(&storage, &identity, next, 261, hash, 256, 1024 * 1024)
            .unwrap()
            .0
            .len(),
        0
    );
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 260);
}

#[test]
fn successor_pages_enforce_wire_budget_and_multi_record_sequences() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 7, "runtime");
    let first = successor_segment(
        &key,
        1,
        2,
        Some(genesis.segment_hash().unwrap()),
        7,
        "runtime",
    );
    let second = successor_segment(
        &key,
        3,
        1,
        Some(first.segment_hash().unwrap()),
        7,
        "runtime",
    );
    insert_successor(&storage, &identity, &second);
    insert_successor(&storage, &identity, &first);
    let bytes = first.wire_bytes().unwrap().len();
    let (page, next, hash) = successor_page(
        &storage,
        &identity,
        1,
        4,
        genesis.segment_hash().unwrap(),
        256,
        bytes,
    )
    .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(next, 3);
    let (page, next, hash) =
        successor_page(&storage, &identity, next, 4, hash, 256, bytes).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(next, 4);
    assert_eq!(hash, second.segment_hash().unwrap());
    assert!(
        successor_page(
            &storage,
            &identity,
            1,
            4,
            genesis.segment_hash().unwrap(),
            256,
            bytes - 1
        )
        .is_err()
    );
    assert!(
        successor_page(
            &storage,
            &identity,
            1,
            4,
            genesis.segment_hash().unwrap(),
            257,
            1024 * 1024
        )
        .is_err()
    );
    assert!(
        successor_page(
            &storage,
            &identity,
            1,
            4,
            genesis.segment_hash().unwrap(),
            256,
            1024 * 1024 + 1
        )
        .is_err()
    );
}

#[test]
fn successor_pages_reject_missing_fork_duplicate_wrong_epoch_and_unindexed_legacy() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 7, "runtime");
    let first = successor_segment(
        &key,
        1,
        1,
        Some(genesis.segment_hash().unwrap()),
        7,
        "runtime",
    );
    insert_successor(&storage, &identity, &first);
    let other_epoch = successor_segment(
        &key,
        2,
        1,
        Some(first.segment_hash().unwrap()),
        8,
        "runtime",
    );
    insert_successor(&storage, &identity, &other_epoch);
    let error = successor_page(
        &storage,
        &identity,
        1,
        3,
        genesis.segment_hash().unwrap(),
        1,
        1024 * 1024,
    )
    .unwrap_err();
    assert!(error.0.contains("missing"), "{error:?}");
    let fork = successor_segment(&key, 2, 1, Some([77; 32]), 7, "runtime");
    insert_successor(&storage, &identity, &fork);
    let error = successor_page(
        &storage,
        &identity,
        1,
        3,
        genesis.segment_hash().unwrap(),
        256,
        1024 * 1024,
    )
    .unwrap_err();
    assert!(error.0.contains("predecessor"), "{error:?}");
    with_connection(&storage, |connection| {
        connection
            .execute(
                "DELETE FROM source_delivery_journal WHERE source_epoch=7 AND first_sequence=2",
                [],
            )
            .unwrap();
    });
    let duplicate = successor_segment(
        &key,
        1,
        2,
        Some(genesis.segment_hash().unwrap()),
        7,
        "runtime",
    );
    insert_successor(&storage, &identity, &duplicate);
    let error = successor_page(
        &storage,
        &identity,
        1,
        3,
        genesis.segment_hash().unwrap(),
        1,
        1024 * 1024,
    )
    .unwrap_err();
    assert!(
        error.0.contains("duplicate") || error.0.contains("crosses"),
        "{error:?}"
    );
    let legacy_id = "legacy";
    insert(
        &storage,
        legacy_id,
        b"old",
        &serde_json::to_vec(&identity).unwrap(),
    );
    let error = successor_page(
        &storage,
        &identity,
        1,
        3,
        genesis.segment_hash().unwrap(),
        256,
        1024 * 1024,
    )
    .unwrap_err();
    assert!(error.0.contains("legacy order repair"), "{error:?}");
    assert_eq!(
        storage
            .source_delivery_journal_by_ids(&ids(&[legacy_id]), 3)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn successor_pages_reject_oversized_wire_before_decoding_and_leave_journal_intact() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 7, "runtime");
    let first = successor_segment(
        &key,
        1,
        1,
        Some(genesis.segment_hash().unwrap()),
        7,
        "runtime",
    );
    insert_successor(&storage, &identity, &first);
    with_connection(&storage, |connection| {
        connection
            .execute(
                "UPDATE source_delivery_journal SET wire=zeroblob(?1) WHERE first_sequence=1",
                [1024 * 1024 + 1],
            )
            .unwrap();
    });
    let error = successor_page(
        &storage,
        &identity,
        1,
        2,
        genesis.segment_hash().unwrap(),
        256,
        1024 * 1024,
    )
    .unwrap_err();
    assert!(error.0.contains("bounds"), "{error:?}");
    with_connection(&storage, |connection| {
        assert_eq!(
            connection
                .query_row(
                    "SELECT length(wire) FROM source_delivery_journal WHERE first_sequence=1",
                    [],
                    |row| row.get::<_, usize>(0)
                )
                .unwrap(),
            1024 * 1024 + 1
        )
    });
}

#[test]
fn successor_preflight_and_blob_reads_use_one_snapshot_with_concurrent_writer() {
    use super::source_journal_exact::{SuccessorQuery, successors_in_snapshot};
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 7, "runtime");
    let first = successor_segment(
        &key,
        1,
        1,
        Some(genesis.segment_hash().unwrap()),
        7,
        "runtime",
    );
    insert_successor(&storage, &identity, &first);
    let writer = Connection::open(directory.path().join(SQLITE_FILE)).unwrap();
    let mut backend = storage.lock_backend();
    let connection = sqlite_connection(&mut backend).unwrap().unwrap();
    let query = SuccessorQuery {
        cluster_id: xp_test_fixtures::primary_cluster_id(),
        identity: &identity,
        epoch: 7,
        stream: "runtime",
        first_sequence: 1,
        end_sequence_exclusive: 2,
        previous_hash: genesis.segment_hash().unwrap(),
        max_segments: 1,
        max_wire_bytes: 1024 * 1024,
    };
    let (rows, next, hash) = successors_in_snapshot(connection, &query, || {
        writer
            .execute(
                "DELETE FROM source_delivery_journal WHERE first_sequence=1",
                [],
            )
            .unwrap();
    })
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].wire, first.wire_bytes().unwrap());
    assert_eq!(next, 2);
    assert_eq!(hash, first.segment_hash().unwrap());
    assert_eq!(
        writer
            .query_row("SELECT COUNT(*) FROM source_delivery_journal", [], |row| {
                row.get::<_, u64>(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn successor_preflights_entire_page_before_decoding_first_identity() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 7, "runtime");
    let first = successor_segment(
        &key,
        1,
        1,
        Some(genesis.segment_hash().unwrap()),
        7,
        "runtime",
    );
    let second = successor_segment(
        &key,
        2,
        1,
        Some(first.segment_hash().unwrap()),
        7,
        "runtime",
    );
    insert_successor(&storage, &identity, &first);
    insert_successor(&storage, &identity, &second);
    with_connection(&storage, |connection| {
        connection
            .execute(
                "UPDATE source_delivery_journal SET identity=?1 WHERE first_sequence=1",
                [b"not json".as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE source_delivery_journal SET wire=zeroblob(?1) WHERE first_sequence=2",
                [1024 * 1024 + 1],
            )
            .unwrap();
    });
    let error = successor_page(
        &storage,
        &identity,
        1,
        3,
        genesis.segment_hash().unwrap(),
        256,
        1024 * 1024,
    )
    .unwrap_err();
    assert!(error.0.contains("bounds"), "{error:?}");
    with_connection(&storage, |connection| {
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM source_delivery_journal", [], |row| {
                    row.get::<_, u64>(0)
                })
                .unwrap(),
            2
        )
    });
}

#[test]
fn successor_rejects_mismatched_index_and_pinned_signing_identity() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 7, "runtime");
    let first = successor_segment(
        &key,
        1,
        1,
        Some(genesis.segment_hash().unwrap()),
        7,
        "runtime",
    );
    insert_successor(&storage, &identity, &first);
    let other_key = ed25519_dalek::SigningKey::from_bytes(&[12; 32]);
    let other_identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("source".to_owned()).unwrap(),
        Ed25519PublicKey::from_bytes(other_key.verifying_key().to_bytes()).unwrap(),
        X25519PublicKey::from_bytes([8; 32]).unwrap(),
    )
    .unwrap();
    assert!(
        successor_page(
            &storage,
            &other_identity,
            1,
            2,
            genesis.segment_hash().unwrap(),
            1,
            1024 * 1024
        )
        .is_err()
    );
    with_connection(&storage, |connection| {
        connection
            .execute(
                "UPDATE source_delivery_journal SET first_sequence=2 WHERE first_sequence=1",
                [],
            )
            .unwrap();
    });
    let error = successor_page(
        &storage,
        &identity,
        2,
        3,
        genesis.segment_hash().unwrap(),
        1,
        1024 * 1024,
    )
    .unwrap_err();
    assert!(error.0.contains("scope"), "{error:?}");
}

#[test]
fn successor_can_read_indexed_signed_epoch_zero_without_treating_it_as_legacy() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 0, "runtime");
    let first = successor_segment(
        &key,
        1,
        1,
        Some(genesis.segment_hash().unwrap()),
        0,
        "runtime",
    );
    insert_successor(&storage, &identity, &first);
    let (rows, next, hash) = storage
        .source_delivery_journal_successors(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            0,
            "runtime",
            1,
            2,
            genesis.segment_hash().unwrap(),
            1,
            1024 * 1024,
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(next, 2);
    assert_eq!(hash, first.segment_hash().unwrap());
}

#[test]
fn successor_rejects_oversized_stored_id_without_reading_wire() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 7, "runtime");
    let first = successor_segment(
        &key,
        1,
        1,
        Some(genesis.segment_hash().unwrap()),
        7,
        "runtime",
    );
    insert_successor(&storage, &identity, &first);
    with_connection(&storage, |connection| {
        connection
            .execute(
                "UPDATE source_delivery_journal SET id=?1, identity=?2 WHERE first_sequence=1",
                params!["x".repeat(1024 * 1024), b"invalid json".as_slice()],
            )
            .unwrap();
    });
    let error = successor_page(
        &storage,
        &identity,
        1,
        2,
        genesis.segment_hash().unwrap(),
        1,
        1024 * 1024,
    )
    .unwrap_err();
    assert!(error.0.contains("id length"), "{error:?}");
}

#[test]
fn successor_reports_row_removed_between_pages_without_undoing_previous_page() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let (key, identity) = successor_identity();
    let genesis = successor_segment(&key, 0, 1, None, 7, "runtime");
    let first = successor_segment(
        &key,
        1,
        1,
        Some(genesis.segment_hash().unwrap()),
        7,
        "runtime",
    );
    let second = successor_segment(
        &key,
        2,
        1,
        Some(first.segment_hash().unwrap()),
        7,
        "runtime",
    );
    insert_successor(&storage, &identity, &first);
    insert_successor(&storage, &identity, &second);
    let (page, next, hash) = successor_page(
        &storage,
        &identity,
        1,
        3,
        genesis.segment_hash().unwrap(),
        1,
        1024 * 1024,
    )
    .unwrap();
    assert_eq!(page.len(), 1);
    with_connection(&storage, |connection| {
        connection
            .execute(
                "DELETE FROM source_delivery_journal WHERE first_sequence=2",
                [],
            )
            .unwrap();
    });
    let error = successor_page(&storage, &identity, next, 3, hash, 1, 1024 * 1024).unwrap_err();
    assert!(error.0.contains("missing"), "{error:?}");
    assert_eq!(storage.source_delivery_journal().unwrap().len(), 1);
}
