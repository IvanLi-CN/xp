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
fn repair_selection_never_skips_same_stream_predecessors() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity(&signing_key);
    let ready = vec!["repository-a".to_owned(), "repository-b".to_owned()];
    let mut primary =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    primary.snapshot.external_history = false;
    let mut previous_hash = None;
    let mut segments = Vec::new();

    for sequence in 0_u64..66 {
        let signed = CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                format!("runtime:{sequence}").into_bytes(),
                format!("sample:{sequence}").into_bytes(),
                false,
            )],
            previous_hash,
            100 + sequence,
            100 + sequence,
        )
        .expect("canonical segment")
        .sign(&signing_key)
        .expect("signed segment");
        previous_hash = Some(signed.segment_hash().expect("segment hash"));
        let id = match sequence {
            0 => "000".to_owned(),
            1 => "001".to_owned(),
            2 => "z02".to_owned(),
            3 => "z03".to_owned(),
            _ => format!("{:03}", sequence - 2),
        };
        segments.push(StoredSegment {
            id,
            closed_at_unix_seconds: 100 + sequence,
            identity: source_identity.clone(),
            wire: signed.wire_bytes().expect("wire"),
        });
    }
    primary.snapshot.segments = segments;

    let summary = primary.replication_summary().expect("summary");
    let standby_temporary = tempfile::tempdir().expect("standby temporary directory");
    let standby = RepositoryReplicaRuntime::load(HistoryStorage::open(standby_temporary.path()))
        .expect("runtime");
    let missing = standby
        .missing_segment_ids(&summary, false)
        .expect("missing segments");
    assert_eq!(missing.len(), 64);

    let repair = primary.repair_batch(&missing).expect("repair batch");
    let mut standby = standby;
    for segment in repair.segments {
        standby
            .receive_wire_from_repository(
                "cluster-a",
                &segment.identity,
                &segment.wire,
                200,
                &ready,
                "repository-b",
            )
            .expect("repair segments must be accepted in source cursor order");
    }
}
