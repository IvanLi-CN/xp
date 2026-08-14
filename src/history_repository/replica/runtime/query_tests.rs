use ed25519_dalek::SigningKey;

use super::*;
use crate::{
    history_sync::{CanonicalSegment, Cursor, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
        query::{Completeness, HistoryQuery},
        replica::RepositoryReplicaGap,
    },
};

#[test]
fn repository_query_scopes_records_and_coverage_to_the_subject_node() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity");
    let segment = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 0).expect("cursor"),
        vec![
            SyncRecord::new(
                "subject-a",
                "node-a",
                "runtime.v1",
                1,
                b"a".to_vec(),
                b"a".to_vec(),
                false,
            ),
            SyncRecord::new(
                "subject-b",
                "node-a",
                "runtime.v1",
                1,
                b"b".to_vec(),
                b"b".to_vec(),
                false,
            ),
        ],
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
            HistoryQuery::new(100, 101, 10)
                .expect("query")
                .with_subject_node_id(Some("subject-a"))
                .expect("subject filter"),
            LocalQueryMetadata::current_window(102),
        )
        .expect("filtered query");

    assert_eq!(response.records.len(), 1);
    let coverage = response.plan().coverage().expect("filtered coverage");
    assert_eq!(coverage.observed().start_unix_seconds(), 101);
    assert_eq!(coverage.observed().end_unix_seconds(), 101);
}

#[test]
fn a_source_backpressure_gap_makes_a_fully_covered_query_partial() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity");
    let first = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 0).expect("cursor"),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "runtime.v1",
            1,
            b"first".to_vec(),
            b"first".to_vec(),
            false,
        )],
        None,
        100,
        100,
    )
    .expect("first segment")
    .sign(&key)
    .expect("first signature");
    let second = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 1).expect("cursor"),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "runtime.v1",
            1,
            b"second".to_vec(),
            b"second".to_vec(),
            false,
        )],
        Some(first.segment_hash().expect("first hash")),
        102,
        102,
    )
    .expect("second segment")
    .sign(&key)
    .expect("second signature");
    let mut runtime =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    for segment in [&first, &second] {
        runtime
            .receive_wire(
                "cluster-a",
                &identity,
                &segment.wire_bytes().expect("wire"),
                103,
            )
            .expect("repository accepts segment");
    }
    runtime
        .merge_replica_gaps(&[RepositoryReplicaGap {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
            first_sequence: 1,
            last_sequence: 1,
            start_unix_seconds: 101,
            end_unix_seconds: 101,
            permanent: true,
        }])
        .expect("merge source backpressure gap");
    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(100, 102, 10)
                .expect("query")
                .with_subject_node_id(Some("node-a"))
                .expect("subject filter"),
            LocalQueryMetadata::current_window(103),
        )
        .expect("partial query");
    assert_eq!(response.plan().completeness(), Completeness::Partial);
    assert_eq!(response.plan().gaps().len(), 1);
}

#[test]
fn an_incomplete_aggregate_for_another_subject_does_not_degrade_this_query() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
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
            b"subject-a".to_vec(),
            b"sample".to_vec(),
            false,
        )],
        None,
        100,
        100,
    )
    .expect("segment")
    .sign(&key)
    .expect("signature");
    let mut runtime =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment.wire_bytes().expect("wire"),
            100,
        )
        .expect("repository accepts subject-a segment");
    runtime.snapshot.records.push(StoredRecord {
        observed_at_unix_seconds: 100,
        received_at_unix_seconds: 100,
        source_node_id: "node-b".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        sequence: 0,
        subject_node_id: "subject-b".to_owned(),
        observer_node_id: "node-b".to_owned(),
        schema_id: "runtime.v1".to_owned(),
        schema_version: 1,
        record_key: b"aggregate".to_vec(),
        payload: serde_json::to_vec(&serde_json::json!({
            "algorithm": "sha256",
            "resolution": "hour",
            "bucket_start_unix_seconds": 100,
            "bucket_end_unix_seconds": 100,
            "record_count": 1,
            "first_sequence": 0,
            "last_sequence": 0,
            "payload_sha256": "00".repeat(32),
            "complete": false,
            "anonymized_identifier": null,
        }))
        .expect("aggregate payload"),
        tombstone: false,
    });

    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(100, 100, 10)
                .expect("query")
                .with_subject_node_id(Some("subject-a"))
                .expect("subject filter"),
            LocalQueryMetadata::current_window(100),
        )
        .expect("query result");

    assert_eq!(
        response.plan().completeness(),
        Completeness::Complete,
        "{:#?}",
        response.plan()
    );
    assert!(response.plan().gaps().is_empty());
}
