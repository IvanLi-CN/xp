use super::{RepositoryReplicaRuntime, StoredSegment};
use crate::{
    history_sync::{CanonicalSegment, Cursor, ProtocolError, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
        replica::RepositoryRuntimeError,
    },
};
use ed25519_dalek::SigningKey;

fn identity() -> RepositoryNodeIdentity {
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

fn load(path: &std::path::Path) -> RepositoryReplicaRuntime {
    RepositoryReplicaRuntime::load(HistoryStorage::open(path)).expect("runtime")
}

fn signed_segment(stream: &str, tombstone: bool) -> Vec<u8> {
    CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 1, stream, 0).expect("cursor"),
        vec![SyncRecord::new(
            "subject-a",
            "node-a",
            "runtime.v1",
            1,
            b"same-key".to_vec(),
            b"payload".to_vec(),
            tombstone,
        )],
        None,
        10,
        11,
    )
    .expect("segment")
    .sign(&SigningKey::from_bytes(&[11; 32]))
    .expect("signed segment")
    .wire_bytes()
    .expect("wire")
}

#[test]
fn snapshot_source_cursor_restarts_tombstones_first() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let repository_identity = identity();
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = false;
    runtime.snapshot.segments = vec![
        StoredSegment {
            id: "a-record".to_owned(),
            closed_at_unix_seconds: 10,
            identity: repository_identity.clone(),
            wire: signed_segment("runtime", false),
        },
        StoredSegment {
            id: "z-tombstone".to_owned(),
            closed_at_unix_seconds: 11,
            identity: repository_identity,
            wire: signed_segment("tombstone", true),
        },
    ];

    let page = runtime
        .stored_segments_page_for_source(Some("s:r:z-tombstone"), 1, Some("node-a"))
        .expect("restart snapshot page");
    assert_eq!(page[0].id, "z-tombstone");
}

#[test]
fn source_scoped_cursor_requires_a_source_filter() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let error = runtime
        .replication_summary_after_for_source(Some("s:t:segment"), false, None)
        .expect_err("unscoped source cursor must fail");
    assert!(matches!(
        error,
        RepositoryRuntimeError::Protocol(ProtocolError::CursorIdentityMismatch)
    ));
}
