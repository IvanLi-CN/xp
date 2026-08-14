use ed25519_dalek::SigningKey;

use super::*;
use crate::{
    history_sync::{CanonicalSegment, Cursor, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
        query::HistoryQuery,
    },
};

#[test]
fn repository_query_budgets_the_serialized_binary_record() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[13; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([14; 32]).expect("relay key"),
    )
    .expect("identity");
    let segment = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 0).expect("cursor"),
        vec![SyncRecord::new(
            "subject-a",
            "node-a",
            "runtime.v1",
            1,
            b"record".to_vec(),
            vec![255; 100 * 1024],
            false,
        )],
        None,
        100,
        101,
    )
    .expect("segment")
    .sign(&key)
    .expect("signature");
    let storage = HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage).expect("runtime");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment.wire_bytes().expect("wire"),
            102,
        )
        .expect("repository accepts records");

    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(100, 101, 10).expect("query"),
            LocalQueryMetadata::current_window(102),
        )
        .expect("bounded query");

    assert_eq!(response.records.len(), 1);
    assert!(
        serde_json::to_vec(&response).expect("JSON response").len() <= MAX_QUERY_RESPONSE_BYTES
    );
}
