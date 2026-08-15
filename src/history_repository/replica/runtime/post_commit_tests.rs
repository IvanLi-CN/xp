use super::*;

#[test]
fn maintenance_failure_keeps_runtime_and_sqlite_committed() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).expect("runtime");
    storage.set_maintenance_failure_for_test(true);

    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment(&key, 0, vec![record(b"committed", false)], None)
                .wire_bytes()
                .expect("wire"),
            11,
        )
        .expect("maintenance failure occurs after the durable commit");
    assert_eq!(storage.repository_history_record_count().expect("count"), 1);
    assert_eq!(
        runtime.runtime_status(12).expect("status").storage_mode,
        "sqlite"
    );

    storage.set_maintenance_failure_for_test(false);
    let restored = RepositoryReplicaRuntime::load(storage).expect("reload committed runtime");
    assert_eq!(
        restored
            .storage
            .repository_history_record_count()
            .expect("reloaded count"),
        1
    );
    assert!(restored.snapshot.receiver.is_some());
}
