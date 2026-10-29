use super::tests::{identity, load, signing_key};
use crate::history_sync::{CanonicalSegment, Cursor, ProtocolError, SyncRecord};

#[test]
fn local_identity_peer_backfill_tombstone_removes_the_peer_stream() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let imported = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "path_health-backfill-node-b", 0).expect("backfill cursor"),
        vec![SyncRecord::new(
            "node-b",
            "node-b",
            "path_health.v1",
            1,
            b"node-history:node:node-b:mesh:peer-d:2026-08-14T00:00:00Z".to_vec(),
            b"peer history".to_vec(),
            false,
        )],
        None,
        10,
        10,
    )
    .expect("peer backfill")
    .sign(&key)
    .expect("sign peer backfill");
    let tombstone = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "tombstone-backfill-node-b", 0)
            .expect("backfill tombstone cursor"),
        vec![SyncRecord::new(
            "node-b",
            "node-b",
            "path_health.v1",
            1,
            b"node-history:node:node-b:".to_vec(),
            b"deleted".to_vec(),
            true,
        )],
        None,
        11,
        11,
    )
    .expect("peer backfill tombstone")
    .sign(&key)
    .expect("sign peer backfill tombstone");

    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &imported.wire_bytes().expect("backfill wire"),
            10,
        )
        .expect("persist peer backfill");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &tombstone.wire_bytes().expect("tombstone wire"),
            11,
        )
        .expect("delete peer backfill history");
    assert!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 8)
            .expect("read records")
            .is_empty()
    );
}

#[test]
fn malformed_backfill_tombstone_stream_is_rejected_before_deletion() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let malformed = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime-backfill-node-a", 0).expect("backfill cursor"),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "runtime.v1",
            1,
            b"node-history:node:node-a:live".to_vec(),
            b"deleted".to_vec(),
            true,
        )],
        None,
        10,
        10,
    )
    .expect("malformed tombstone segment")
    .sign(&key)
    .expect("sign malformed tombstone");

    let error = runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &malformed.wire_bytes().expect("malformed wire"),
            10,
        )
        .expect_err("malformed tombstone stream must fail closed");
    assert!(matches!(
        error,
        super::RepositoryRuntimeError::Protocol(ProtocolError::CursorIdentityMismatch)
    ));
}
