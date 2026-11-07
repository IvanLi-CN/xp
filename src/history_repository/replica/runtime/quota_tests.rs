use super::{tests::*, *};

#[test]
fn local_quota_persists_without_evicting_retained_history() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let key = signing_key();
    let identity = identity(&key);
    let wire = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 1, "runtime", 0).unwrap(),
        vec![record(b"retained", false)],
        None,
        100,
        100,
    )
    .unwrap()
    .sign(&key)
    .unwrap()
    .wire_bytes()
    .unwrap();
    runtime
        .receive_wire("cluster-a", &identity, &wire, 100)
        .unwrap();
    runtime.set_repository_quota_bytes(9_000_000_000).unwrap();
    assert!(runtime.set_repository_quota_bytes(0).is_err());
    drop(runtime);
    let mut runtime = load(directory.path());
    assert_eq!(
        runtime.runtime_capacity().unwrap().quota_bytes(),
        9_000_000_000
    );
    runtime.set_repository_quota_bytes(1).unwrap();
    let status = serde_json::to_value(runtime.runtime_status(101).unwrap()).unwrap();
    assert_eq!(status["record_count"], 1);
    assert_eq!(
        runtime
            .runtime_capacity()
            .unwrap()
            .history_write_availability(),
        crate::state::history_repository::control::HistoryWriteAvailability::QuotaReached
    );
    assert!(matches!(
        runtime.receive_wire("cluster-a", &identity, &wire, 102),
        Err(RepositoryRuntimeError::WriteStopped(_))
    ));
    drop(runtime);
    let mut runtime = load(directory.path());
    assert_eq!(runtime.runtime_capacity().unwrap().quota_bytes(), 1);
    assert_eq!(
        serde_json::to_value(runtime.runtime_status(103).unwrap()).unwrap()["record_count"],
        1
    );
}
