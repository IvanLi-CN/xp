use ed25519_dalek::SigningKey;

use super::{RepositoryReplicaRuntime, StoredSegment};
use crate::{
    history_sync::{CanonicalSegment, Cursor, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
    },
};

fn identity(signing_key: &SigningKey) -> RepositoryNodeIdentity {
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

#[test]
fn sqlite_summary_keeps_tombstones_first_across_keyset_pages() {
    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let repository_identity = identity(&signing_key);
    let signed = |stream: &str, tombstone: bool| {
        CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 1, stream, 0).expect("cursor"),
            vec![SyncRecord::new(
                "subject-a",
                "node-a",
                "runtime.v1",
                1,
                stream.as_bytes().to_vec(),
                b"payload".to_vec(),
                tombstone,
            )],
            None,
            10,
            11,
        )
        .expect("segment")
        .sign(&signing_key)
        .expect("signed segment")
        .wire_bytes()
        .expect("wire")
    };
    let ordinary = StoredSegment {
        id: "000-ordinary".to_owned(),
        closed_at_unix_seconds: 10,
        identity: repository_identity.clone(),
        wire: signed("ordinary", false),
    };
    let tombstone_wire = signed("tombstone", true);
    let mut segments = vec![ordinary];
    segments.extend((0..256).map(|index| StoredSegment {
        id: format!("tombstone-{index:03}"),
        closed_at_unix_seconds: 11,
        identity: repository_identity.clone(),
        wire: tombstone_wire.clone(),
    }));
    let rows = segments
        .iter()
        .map(StoredSegment::sqlite_row)
        .collect::<Result<Vec<_>, _>>()
        .expect("SQLite rows");
    let runtime =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    runtime
        .storage
        .upsert_repository_history_segments(&rows)
        .expect("store interleaved segment IDs");

    let first = runtime.replication_summary().expect("first summary page");
    assert_eq!(first.segment_ids.len(), 256);
    assert!(
        first
            .segment_ids
            .iter()
            .all(|id| id.starts_with("tombstone-"))
    );
    let cursor = first.next_segment_id.expect("tombstone page cursor");
    assert!(cursor.starts_with("t:"));

    let second = runtime
        .replication_summary_after(Some(&cursor), false)
        .expect("ordinary summary page");
    assert_eq!(second.segment_ids, ["000-ordinary"]);
    assert!(second.next_segment_id.is_none());
}
