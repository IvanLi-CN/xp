use super::super::*;
use super::{identity, load, record, segment, segment_at, signing_key};

#[test]
fn expired_tombstone_is_not_removed_when_a_batch_rolls_back() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let key = signing_key();
    let source = identity(&key);
    let tombstone = segment(&key, 0, vec![record(b"deleted-key", true)], None);
    runtime
        .receive_wire("cluster-a", &source, &tombstone.wire_bytes().unwrap(), 100)
        .unwrap();
    let fresh_at = 100 + TOMBSTONE_HORIZON_SECONDS - 1;
    let fresh = segment_at(
        &key,
        1,
        vec![record(b"unrelated-key", false)],
        Some(tombstone.segment_hash().unwrap()),
        fresh_at,
        fresh_at,
    );
    runtime
        .receive_wire("cluster-a", &source, &fresh.wire_bytes().unwrap(), fresh_at)
        .unwrap();
    let first = segment(
        &key,
        2,
        vec![record(b"new-key", false)],
        Some(fresh.segment_hash().unwrap()),
    );
    let gap = segment(
        &key,
        4,
        vec![record(b"gap-key", false)],
        Some(first.segment_hash().unwrap()),
    );
    let before_control = runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap();
    let before_count = runtime.storage.repository_history_record_count().unwrap();
    assert_eq!(before_count, 2);
    let wires = [first, gap].map(|signed| RepositoryReplicaSegment {
        identity: source.clone(),
        wire: signed.wire_bytes().unwrap(),
    });
    assert!(
        runtime
            .receive_wire_batch_from_repository_with_gaps(
                "cluster-a",
                &wires,
                &[],
                100 + TOMBSTONE_HORIZON_SECONDS,
                &["local".into()],
                "local",
            )
            .is_err()
    );
    assert_eq!(
        runtime.storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        before_control
    );
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        before_count
    );
    drop(runtime);
    let runtime = load(directory.path());
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        before_count
    );
}

#[test]
fn expiring_older_same_key_keeps_newer_tombstone_and_receiver_protection() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let key = signing_key();
    let source = identity(&key);
    let old = segment_at(&key, 0, vec![record(b"deleted-key", true)], None, 100, 100);
    runtime
        .receive_wire("cluster-a", &source, &old.wire_bytes().unwrap(), 100)
        .unwrap();
    let newer = segment_at(
        &key,
        1,
        vec![record(b"deleted-key", true)],
        Some(old.segment_hash().unwrap()),
        200,
        200,
    );
    runtime
        .receive_wire("cluster-a", &source, &newer.wire_bytes().unwrap(), 200)
        .unwrap();
    let now = 100 + TOMBSTONE_HORIZON_SECONDS;
    let fresh = segment_at(
        &key,
        2,
        vec![record(b"unrelated-key", false)],
        Some(newer.segment_hash().unwrap()),
        now,
        now,
    );
    runtime
        .receive_wire("cluster-a", &source, &fresh.wire_bytes().unwrap(), now)
        .unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    let sequences = connection
        .prepare("SELECT sequence FROM repository_history_records WHERE is_tombstone=1")
        .unwrap()
        .query_map([], |row| row.get::<_, u64>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(sequences, vec![1]);
    let cursor = Cursor::new("node-a", 7, "runtime", 3).unwrap();
    assert!(
        runtime
            .receiver
            .as_ref()
            .unwrap()
            .is_tombstoned(&cursor, &record(b"deleted-key", false))
    );
    drop(connection);
    drop(runtime);
    let runtime = load(directory.path());
    assert!(
        runtime
            .receiver
            .as_ref()
            .unwrap()
            .is_tombstoned(&cursor, &record(b"deleted-key", false))
    );
}
