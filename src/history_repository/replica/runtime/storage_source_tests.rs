use super::{RepositoryReplicaRuntime, StoredSegment};
use crate::{
    history_sync::{CanonicalSegment, Cursor, ProtocolError, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
        replica::RepositoryRuntimeError,
    },
    state::history_storage::RepositoryHistorySegmentRow,
};
use ed25519_dalek::SigningKey;

fn identity(node_id: &str) -> RepositoryNodeIdentity {
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from(node_id.to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

fn load(path: &std::path::Path) -> RepositoryReplicaRuntime {
    RepositoryReplicaRuntime::load(HistoryStorage::open(path)).expect("runtime")
}

fn signed_segment(source_node_id: &str, stream: &str, sequence: u64, tombstone: bool) -> Vec<u8> {
    CanonicalSegment::new(
        "cluster-a",
        Cursor::new(source_node_id, 1, stream, sequence).expect("cursor"),
        vec![SyncRecord::new(
            "subject-a",
            source_node_id,
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
    let repository_identity = identity("node-a");
    let other_identity = identity("node-b");
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = false;
    runtime.snapshot.segments = vec![
        StoredSegment {
            id: "a-record".to_owned(),
            closed_at_unix_seconds: 10,
            identity: repository_identity.clone(),
            wire: signed_segment("node-a", "runtime", 1, false),
        },
        StoredSegment {
            id: "b-record".to_owned(),
            closed_at_unix_seconds: 11,
            identity: repository_identity.clone(),
            wire: signed_segment("node-a", "runtime", 2, false),
        },
        StoredSegment {
            id: "z-tombstone".to_owned(),
            closed_at_unix_seconds: 11,
            identity: repository_identity,
            wire: signed_segment("node-a", "tombstone", 0, true),
        },
        StoredSegment {
            id: "other-source".to_owned(),
            closed_at_unix_seconds: 12,
            identity: other_identity,
            wire: signed_segment("node-b", "runtime", 0, false),
        },
    ];

    let first = runtime
        .stored_segments_page_for_source(None, 1, Some("node-a"))
        .expect("first snapshot page");
    assert_eq!(first[0].id, "z-tombstone");
    let second = runtime
        .stored_segments_page_for_source(Some("s:t:z-tombstone"), 1, Some("node-a"))
        .expect("second snapshot page");
    assert_eq!(second[0].id, "a-record");
    let page = runtime
        .stored_segments_page_for_source(Some("s:r:a-record"), 1, Some("node-a"))
        .expect("restart snapshot page");
    assert_eq!(page[0].id, "b-record");
    let restarted = runtime
        .stored_segments_page_for_source(Some("s:r:z-tombstone"), 1, Some("node-a"))
        .expect("restart mismatched snapshot page");
    assert_eq!(restarted[0].id, "z-tombstone");
    let restarted_from_legacy = runtime
        .stored_segments_page_for_source(Some("r:b-record"), 1, Some("node-a"))
        .expect("restart legacy global cursor page");
    assert_eq!(restarted_from_legacy[0].id, "z-tombstone");
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

#[test]
fn source_scoped_summary_restarts_after_history_revision_changes() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    runtime
        .storage
        .upsert_repository_history_segments(&[RepositoryHistorySegmentRow {
            id: "ordinary".to_owned(),
            closed_at_unix_seconds: 1,
            contains_tombstone: false,
            source_node_id: "node-a".to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            first_sequence: 1,
            payload: Vec::new(),
        }])
        .expect("ordinary segment");
    let revision = runtime
        .storage
        .repository_history_segment_revision("node-a")
        .expect("initial segment revision")
        .to_string();
    runtime
        .storage
        .upsert_repository_history_segments(&[RepositoryHistorySegmentRow {
            id: "other-source".to_owned(),
            closed_at_unix_seconds: 2,
            contains_tombstone: false,
            source_node_id: "node-b".to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            first_sequence: 1,
            payload: Vec::new(),
        }])
        .expect("unrelated source segment");
    assert_eq!(
        runtime
            .storage
            .repository_history_segment_revision("node-a")
            .expect("source revision after unrelated write")
            .to_string(),
        revision
    );
    runtime
        .storage
        .upsert_repository_history_segments(&[RepositoryHistorySegmentRow {
            id: "tombstone".to_owned(),
            closed_at_unix_seconds: 3,
            contains_tombstone: true,
            source_node_id: "node-a".to_owned(),
            source_epoch: 1,
            stream: "tombstone".to_owned(),
            first_sequence: 0,
            payload: Vec::new(),
        }])
        .expect("new tombstone segment");

    let summary = runtime
        .replication_summary_after_for_source_with_revision(
            Some("s:r:ordinary"),
            false,
            Some("node-a"),
            Some(&revision),
        )
        .expect("restarted source summary");
    assert_eq!(summary.segment_ids, ["tombstone", "ordinary"]);
    assert_ne!(
        summary.source_summary_revision.as_deref(),
        Some(revision.as_str())
    );
}
