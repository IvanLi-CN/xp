use std::collections::{BTreeMap, BTreeSet};

use super::{LocalSourceState, LocalSourceStreamState, RepositoryReplicaRuntime};
use crate::history_sync::SyncRecord;
use crate::state::history_repository::identity::{
    Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey,
};
use ed25519_dalek::SigningKey;

#[test]
fn stale_repository_rebuild_rotates_the_durable_source_epoch_before_resetting_sequences() {
    let mut state = LocalSourceState {
        epoch: 7,
        streams: BTreeMap::from([("runtime".to_owned(), LocalSourceStreamState::default())]),
        ..LocalSourceState::default()
    };
    state
        .rotate_after_repository_rebuild()
        .expect("rotate source epoch");
    assert_eq!(state.epoch, 8);
    assert!(state.streams.is_empty());
}

#[test]
fn stale_repository_rebuild_rejects_exhausted_source_epoch() {
    let mut state = LocalSourceState {
        epoch: i64::MAX as u64,
        ..LocalSourceState::default()
    };

    assert!(state.rotate_after_repository_rebuild().is_err());
    assert_eq!(state.epoch, i64::MAX as u64);
}

#[test]
fn disjoint_backpressure_ranges_remain_independent() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::empty(storage);
    runtime.snapshot.local_source.epoch = 7;

    runtime.record_local_source_backpressure_gap("runtime", 8, 8, 100);
    runtime.record_local_source_backpressure_gap("runtime", 10, 10, 120);

    let gaps = runtime.local_source_backpressure_gaps("node-a");
    assert_eq!(gaps.len(), 2);
    assert_eq!(gaps[0].stream, "runtime");
    assert_eq!((gaps[0].first_sequence, gaps[0].last_sequence), (8, 8));
    assert_eq!((gaps[1].first_sequence, gaps[1].last_sequence), (10, 10));
    assert!(gaps.iter().all(|gap| gap.permanent));
}

#[test]
fn backpressure_gap_requests_rotate_across_the_repair_limit() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::empty(storage.clone());
    runtime.snapshot.local_source.epoch = 7;

    for index in 0..65 {
        let sequence = index * 2 + 1;
        runtime.record_local_source_backpressure_gap("runtime", sequence, sequence, sequence);
    }

    let first_page = runtime.local_source_backpressure_gaps("node-a");
    assert!(
        runtime
            .snapshot
            .local_source
            .backpressure_gap_cursor
            .is_none()
    );
    runtime
        .commit_local_source_gap_page(&first_page)
        .expect("commit first gap page");
    runtime
        .persist_control_state()
        .expect("persist first gap cursor");
    let mut restarted = RepositoryReplicaRuntime::load(storage).expect("reload runtime");
    let second_page = restarted.local_source_backpressure_gaps("node-a");
    assert_eq!(first_page.len(), 64);
    assert_eq!(second_page.len(), 64);
    let all_sequences = first_page
        .iter()
        .chain(&second_page)
        .map(|gap| gap.first_sequence)
        .collect::<BTreeSet<_>>();
    assert_eq!(all_sequences.len(), 65);
    assert!(all_sequences.contains(&129));
}

#[test]
fn failed_gap_delivery_does_not_advance_the_page_cursor() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::empty(storage);
    runtime.snapshot.local_source.epoch = 7;
    for index in 0..65 {
        let sequence = index * 2 + 1;
        runtime.record_local_source_backpressure_gap("runtime", sequence, sequence, sequence);
    }

    let first_page = runtime.local_source_backpressure_gaps("node-a");
    assert_eq!(first_page.len(), 64);
    assert!(
        runtime
            .snapshot
            .local_source
            .backpressure_gap_cursor
            .is_none()
    );
    runtime
        .commit_local_source_gap_page(&first_page)
        .expect("commit delivered gap page");
    let second_page = runtime.local_source_backpressure_gaps("node-a");
    assert_eq!(second_page.len(), 64);
    assert!(second_page.iter().any(|gap| gap.first_sequence == 129));
}

#[test]
fn backpressure_gap_page_prioritizes_the_gap_before_the_pending_segment() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::empty(storage);
    runtime.snapshot.local_source.epoch = 7;
    runtime.snapshot.local_source.streams.insert(
        "runtime".to_owned(),
        LocalSourceStreamState {
            next_sequence: 130,
            ..LocalSourceStreamState::default()
        },
    );
    runtime.record_local_source_backpressure_gap("runtime", 120, 129, 100);
    for index in 0..64 {
        let sequence = index * 2 + 1_000;
        runtime.record_local_source_backpressure_gap("runtime", sequence, sequence, 100);
    }
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).expect("public key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity");
    let pending = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity,
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:130".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            100,
        )
        .expect("queue segment")
        .expect("pending segment");

    let gaps = runtime.local_source_gaps_for_segments("node-a", &[pending]);
    assert_eq!(gaps.len(), 64);
    assert!(
        gaps.iter()
            .any(|gap| (gap.first_sequence, gap.last_sequence) == (120, 129))
    );
}

#[test]
fn failed_sqlite_control_write_reports_read_only_degradation() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).expect("runtime");
    storage
        .set_query_only_for_test(true)
        .expect("enable SQLite write failure");

    assert!(
        runtime
            .record_local_source_collector_delivery("repository-a", "repository-a", false)
            .is_err()
    );
    assert_eq!(
        runtime
            .runtime_status(12)
            .expect("degraded status remains readable")
            .storage_mode,
        "sqlite_degraded"
    );
}
