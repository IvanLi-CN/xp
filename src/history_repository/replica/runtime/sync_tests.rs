use ed25519_dalek::SigningKey;

use super::{RepositoryRepairBatch, RepositoryReplicaRuntime, StoredSegment};
use crate::{
    history_sync::{CanonicalSegment, Cursor, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
        replica::ReplicaWork,
    },
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
    runtime.snapshot.segments = (0_u8..128)
        .map(|sequence| StoredSegment {
            id: format!("segment-{sequence}"),
            identity: identity.clone(),
            wire: vec![sequence],
        })
        .collect();

    let first = runtime
        .relay_batch("repository-b")
        .expect("first bounded relay page");
    assert_eq!(first.batch.segments.len(), 64);
    assert_eq!(first.batch.segments[0].wire, vec![0]);
    assert_eq!(first.batch.segments[63].wire, vec![63]);
    let next_segment_id = first
        .next_segment_id()
        .expect("next retained segment")
        .to_owned();
    runtime.snapshot.segments.drain(..64);
    runtime
        .snapshot
        .segments
        .extend((128_u8..192).map(|sequence| StoredSegment {
            id: format!("segment-{sequence}"),
            identity: identity.clone(),
            wire: vec![sequence],
        }));
    runtime
        .record_relay_batch_delivered("repository-b", Some(&next_segment_id))
        .expect("persist relay cursor");

    let restored = load(temporary.path());
    let second = restored
        .relay_batch("repository-b")
        .expect("second bounded relay page");
    assert_eq!(second.batch.segments.len(), 64);
    assert_eq!(second.batch.segments[0].wire, vec![64]);
    assert_eq!(second.batch.segments[63].wire, vec![127]);
}

#[test]
fn repair_batch_accepts_a_pre_gap_metadata_payload() {
    let batch = serde_json::from_slice::<RepositoryRepairBatch>(br#"{"segments":[]}"#)
        .expect("legacy repair payload remains readable");
    assert!(batch.segments.is_empty());
    assert!(batch.gaps.is_empty());
}

#[test]
fn active_tombstone_acknowledgements_retry_after_restart_until_delivered() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let tombstone = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 0).expect("cursor"),
        vec![SyncRecord::new(
            "subject-a",
            "node-a",
            "runtime.v1",
            1,
            b"dead".to_vec(),
            b"sample".to_vec(),
            true,
        )],
        None,
        10,
        11,
    )
    .expect("segment")
    .sign(&signing_key)
    .expect("signature");
    let ready = vec!["repo-a".to_owned(), "repo-b".to_owned()];
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire_from_repository(
            "cluster-a",
            &identity(),
            &tombstone.wire_bytes().expect("wire"),
            100,
            &ready,
            "repo-a",
        )
        .expect("tombstone");

    let failed_delivery = runtime
        .tombstone_acknowledgement_page("repo-a")
        .expect("pending acknowledgement");
    assert_eq!(failed_delivery.acknowledgements().len(), 1);
    let pending_key = failed_delivery.acknowledgements()[0].key.clone();

    let mut restored = load(temporary.path());
    let retry = restored
        .tombstone_acknowledgement_page("repo-a")
        .expect("restart preserves pending acknowledgement");
    assert_eq!(retry.acknowledgements().len(), 1);
    assert_eq!(retry.acknowledgements()[0].key, pending_key);

    restored
        .record_tombstone_acknowledgement_delivery(retry.next_cursor())
        .expect("record complete fanout");
    let replay = load(temporary.path())
        .tombstone_acknowledgement_page("repo-a")
        .expect("active tombstones continue to cover new ready peers");
    assert_eq!(replay.acknowledgements().len(), 1);
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

#[test]
fn deep_verification_rotates_through_every_ready_peer_before_completing() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let ready = [
        "repository-local".to_owned(),
        "repository-a".to_owned(),
        "repository-b".to_owned(),
        "repository-c".to_owned(),
        "repository-d".to_owned(),
        "repository-e".to_owned(),
        "repository-f".to_owned(),
    ];
    let mut runtime = load(temporary.path());
    let first = runtime
        .next_replication_peers(&ready, "repository-local", 4)
        .expect("first peer page");
    assert_eq!(
        first,
        [
            "repository-a",
            "repository-b",
            "repository-c",
            "repository-d"
        ]
    );
    for peer in &first {
        assert!(
            !runtime
                .record_direct_peer_deep_verification(
                    peer,
                    &ready,
                    "repository-local",
                    ReplicaWork::DeepVerification,
                )
                .expect("partial deep verification")
        );
    }

    let mut restored = load(temporary.path());
    let second = restored
        .next_replication_peers(&ready, "repository-local", 4)
        .expect("second peer page");
    assert_eq!(
        second,
        [
            "repository-e",
            "repository-f",
            "repository-a",
            "repository-b"
        ]
    );
    let mut completed = false;
    for peer in &second {
        completed |= restored
            .record_direct_peer_deep_verification(
                peer,
                &ready,
                "repository-local",
                ReplicaWork::DeepVerification,
            )
            .expect("direct peer verification");
    }
    assert!(completed);
}
