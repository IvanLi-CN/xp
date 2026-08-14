use ed25519_dalek::SigningKey;

use super::{RepositoryRepairBatch, RepositoryReplicaRuntime, StoredSegment};
use crate::state::history_repository::{
    HistoryStorage,
    identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
};

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

#[test]
fn relay_repair_pages_through_the_full_bounded_segment_history_after_restart() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let identity = identity();
    let mut runtime = load(temporary.path());
    runtime.snapshot.segments = (0_u8..65)
        .map(|sequence| StoredSegment {
            id: format!("segment-{sequence}"),
            identity: identity.clone(),
            wire: vec![sequence],
        })
        .collect();

    let first = runtime
        .relay_batch("repository-b")
        .expect("first bounded relay page");
    assert_eq!(first.segments.len(), 64);
    assert_eq!(first.segments[0].wire, vec![0]);
    assert_eq!(first.segments[63].wire, vec![63]);
    runtime
        .record_relay_batch_delivered("repository-b", first.segments.len())
        .expect("persist relay cursor");

    let restored = load(temporary.path());
    let second = restored
        .relay_batch("repository-b")
        .expect("second bounded relay page");
    assert_eq!(second.segments.len(), 1);
    assert_eq!(second.segments[0].wire, vec![64]);
}

#[test]
fn repair_batch_accepts_a_pre_gap_metadata_payload() {
    let batch = serde_json::from_slice::<RepositoryRepairBatch>(br#"{"segments":[]}"#)
        .expect("legacy repair payload remains readable");
    assert!(batch.segments.is_empty());
    assert!(batch.gaps.is_empty());
}

#[test]
fn rendezvous_failover_tracks_an_unseen_membership_source() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let ready = vec![
        "repository-a".to_owned(),
        "repository-b".to_owned(),
        "repository-c".to_owned(),
    ];
    let source = "new-source";
    let assignment = super::super::rendezvous_collectors(source, &ready).expect("assignment");
    let standby = assignment.standby().expect("standby").to_owned();
    let mut runtime = load(temporary.path());

    for now in [300, 600, 900] {
        runtime
            .record_stale_collection_cycles(now, &ready, assignment.primary(), &[source.to_owned()])
            .expect("record unseen source failure");
    }

    assert!(
        runtime
            .collects_source(source, &ready, &standby)
            .expect("standby takes an unseen source after three cycles")
    );
}
