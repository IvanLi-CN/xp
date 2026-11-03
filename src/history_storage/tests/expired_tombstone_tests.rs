use super::*;

fn row(sequence: u64, tombstone: bool) -> RepositoryHistoryRecordRow {
    RepositoryHistoryRecordRow {
        source_node_id: "node-a".into(),
        source_epoch: 7,
        stream: "tombstone".into(),
        sequence,
        subject_node_id: "subject".into(),
        observer_node_id: "observer".into(),
        schema_id: "runtime.v1".into(),
        schema_version: 1,
        record_key: b"user:".to_vec(),
        tombstone,
        observed_start_unix_seconds: 10,
        observed_end_unix_seconds: 11,
        received_at_unix_seconds: 12,
        aggregate_complete: Some(true),
        aggregate_start_unix_seconds: None,
        aggregate_end_unix_seconds: None,
        payload: b"old".to_vec(),
    }
}

fn expired(row: &RepositoryHistoryRecordRow) -> RepositoryExpiredTombstone {
    RepositoryExpiredTombstone {
        sequence: Some(row.sequence),
        tombstone: RepositoryHistoryTombstone {
            source_node_id: row.source_node_id.clone(),
            source_epoch: row.source_epoch,
            stream: row.stream.clone(),
            source_stream: None,
            subject_node_id: row.subject_node_id.clone(),
            observer_node_id: row.observer_node_id.clone(),
            schema_id: row.schema_id.clone(),
            schema_version: row.schema_version,
            record_key: row.record_key.clone(),
            // Prefix semantics apply to target deletion, never to expired tombstone row selection.
            prefix: true,
        },
    }
}

fn rows(storage: &HistoryStorage) -> Vec<(String, u64, String, u64, bool, Vec<u8>)> {
    let mut backend = storage.lock_backend();
    let connection = sqlite_connection(&mut backend).unwrap().unwrap();
    let mut statement = connection
        .prepare(
            "SELECT source_node_id,source_epoch,stream,sequence,is_tombstone,payload
             FROM repository_history_records ORDER BY sequence",
        )
        .unwrap();
    let result = statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    result
}

#[test]
fn expired_tombstone_commit_removes_only_exact_tombstones() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let target = row(1, true);
    let mut others: Vec<_> = (2..=10).map(|sequence| row(sequence, true)).collect();
    others[0].source_node_id = "node-b".into();
    others[1].source_epoch = 8;
    others[2].stream = "other".into();
    others[3].subject_node_id = "different-subject".into();
    others[4].observer_node_id = "different-observer".into();
    others[5].schema_id = "different.v1".into();
    others[6].schema_version = 2;
    others[7].record_key = b"user:child".to_vec();
    others[8].tombstone = false;
    let mut initial = vec![target.clone()];
    initial.extend(others);
    storage.upsert_repository_history_records(&initial).unwrap();
    storage
        .commit_repository_replica_mutation(
            RepositoryReplicaMutation {
                expired_tombstones: vec![expired(&target)],
                ..Default::default()
            },
            b"committed",
        )
        .unwrap();
    let saved = rows(&storage);
    assert_eq!(saved.len(), 9);
    assert_eq!(
        saved.iter().map(|row| row.3).collect::<Vec<_>>(),
        (2..=10).collect::<Vec<_>>()
    );
    assert_eq!(
        storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        Some(b"committed".to_vec())
    );
}

#[test]
fn expired_tombstone_and_record_upsert_roll_back_when_checkpoint_fails() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let target = row(1, true);
    let new = row(2, false);
    storage
        .upsert_repository_history_records(&[target.clone()])
        .unwrap();
    storage.write(REPOSITORY_REPLICA_KEY, b"before").unwrap();
    {
        let mut backend = storage.lock_backend();
        let connection = sqlite_connection(&mut backend).unwrap().unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_expiry_checkpoint BEFORE UPDATE ON history_snapshots
             WHEN NEW.key='repository_replica'
             BEGIN SELECT RAISE(ABORT,'injected checkpoint failure'); END;",
            )
            .unwrap();
    }
    assert!(
        storage
            .commit_repository_replica_mutation(
                RepositoryReplicaMutation {
                    expired_tombstones: vec![expired(&target)],
                    records: vec![new],
                    ..Default::default()
                },
                b"after"
            )
            .is_err()
    );
    let saved = rows(&storage);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].3, 1);
    assert!(saved[0].4);
    assert_eq!(
        storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        Some(b"before".to_vec())
    );
}

#[test]
fn expired_tombstone_is_removed_before_same_key_tombstone_is_reinserted() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let old = row(1, true);
    let mut replacement = old.clone();
    replacement.payload = b"replacement".to_vec();
    replacement.received_at_unix_seconds = 100;
    let mut newly_received = row(2, false);
    newly_received.stream = "runtime".into();
    newly_received.record_key = b"user:child".to_vec();
    let mut target = expired(&old).tombstone;
    target.stream = "runtime".into();
    storage
        .upsert_repository_history_records(&[old.clone()])
        .unwrap();
    storage
        .commit_repository_replica_mutation(
            RepositoryReplicaMutation {
                expired_tombstones: vec![expired(&old)],
                records: vec![replacement, newly_received],
                tombstones: vec![target],
                ..Default::default()
            },
            b"after",
        )
        .unwrap();
    let saved = rows(&storage);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].3, 1);
    assert!(saved[0].4);
    assert_eq!(saved[0].5, b"replacement");
    assert_eq!(
        storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        Some(b"after".to_vec())
    );
}

#[test]
fn expiring_older_tombstone_preserves_newer_sequence_with_the_same_key() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let old = row(1, true);
    let newer = row(2, true);
    storage
        .upsert_repository_history_records(&[old.clone(), newer])
        .unwrap();
    storage
        .commit_repository_replica_mutation(
            RepositoryReplicaMutation {
                expired_tombstones: vec![expired(&old)],
                ..Default::default()
            },
            b"after",
        )
        .unwrap();
    let saved = rows(&storage);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].3, 2);
    assert!(saved[0].4);
}

#[test]
fn missing_legacy_sequence_does_not_guess_a_newer_tombstone() {
    let directory = tempfile::tempdir().unwrap();
    let storage = HistoryStorage::open(directory.path());
    let zero = row(0, true);
    let newer = row(2, true);
    storage
        .upsert_repository_history_records(&[zero, newer.clone()])
        .unwrap();
    storage.write(REPOSITORY_REPLICA_KEY, b"before").unwrap();
    let mut ambiguous = expired(&newer);
    ambiguous.sequence = None;
    let error = storage
        .commit_repository_replica_mutation(
            RepositoryReplicaMutation {
                expired_tombstones: vec![ambiguous],
                ..Default::default()
            },
            b"after",
        )
        .unwrap_err();
    assert!(error.to_string().contains("sequence is ambiguous"));
    assert_eq!(
        rows(&storage).iter().map(|row| row.3).collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(
        storage.read(REPOSITORY_REPLICA_KEY).unwrap(),
        Some(b"before".to_vec())
    );
}
