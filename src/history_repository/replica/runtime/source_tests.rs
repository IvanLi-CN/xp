use std::collections::{BTreeMap, BTreeSet};

use super::{LocalSourceState, LocalSourceStreamState, RepositoryReplicaRuntime};

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
