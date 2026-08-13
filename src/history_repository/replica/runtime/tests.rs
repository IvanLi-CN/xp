use ed25519_dalek::SigningKey;

use super::*;
use crate::{
    history_sync::{CanonicalSegment, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        control::{HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES, HistoryWriteAvailability},
        identity::{Ed25519PublicKey, RepositoryNodeId, X25519PublicKey},
        query::Completeness,
    },
};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[11; 32])
}

fn identity(key: &SigningKey) -> RepositoryNodeIdentity {
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

fn record(key: &[u8], tombstone: bool) -> SyncRecord {
    SyncRecord::new(
        "subject-a",
        "node-a",
        "runtime.v1",
        1,
        key.to_vec(),
        b"sample".to_vec(),
        tombstone,
    )
}

fn segment(
    signing_key: &SigningKey,
    sequence: u64,
    records: Vec<SyncRecord>,
    previous: Option<[u8; 32]>,
) -> SignedSegment {
    CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
        records,
        previous,
        10,
        11,
    )
    .expect("segment")
    .sign(signing_key)
    .expect("signature")
}

fn load(path: &std::path::Path) -> RepositoryReplicaRuntime {
    RepositoryReplicaRuntime::load(HistoryStorage::open(path)).expect("runtime")
}

#[test]
fn sqlite_restart_restores_continuous_acknowledgements_and_records() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let first_hash = first.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    let receipt = runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("accepted first segment");
    assert_eq!(receipt.acknowledgement.sequence, 0);

    let mut restored = load(temporary.path());
    let second = segment(&key, 1, vec![record(b"two", false)], Some(first_hash));
    let receipt = restored
        .receive_wire(
            "cluster-a",
            &identity,
            &second.wire_bytes().expect("wire"),
            12,
        )
        .expect("restored receiver accepts next segment");
    assert_eq!(receipt.acknowledgement.sequence, 1);
    let response = restored
        .query(
            "repository-a",
            HistoryQuery::new(11, 11, 10).expect("query"),
            LocalQueryMetadata::current_window(11),
        )
        .expect("query response");
    assert_eq!(response.plan.completeness(), Completeness::Complete);
    assert_eq!(response.plan.repository_id(), Some("repository-a"));
    assert_eq!(response.records.len(), 2);
}

#[test]
fn gaps_do_not_advance_the_persisted_acknowledgement() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let first_hash = first.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("first segment");
    let gap = segment(&key, 2, vec![record(b"three", false)], Some(first_hash));
    assert!(matches!(
        runtime.receive_wire("cluster-a", &identity, &gap.wire_bytes().expect("wire"), 12),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::SequenceGap {
                expected: 1,
                actual: 2,
            }
        ))
    ));
    let restored = load(temporary.path());
    let response = restored
        .query(
            "repository-a",
            HistoryQuery::new(11, 11, 10).expect("query"),
            LocalQueryMetadata::current_window(11),
        )
        .expect("query response");
    assert_eq!(response.plan.watermarks()[0].sequence(), 0);
    assert_eq!(response.plan.gaps().len(), 1);
}

#[test]
fn repaired_segments_clear_gaps_only_after_the_continuous_chain_is_restored() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let first_hash = first.segment_hash().expect("hash");
    let second = segment(&key, 1, vec![record(b"two", false)], Some(first_hash));
    let second_hash = second.segment_hash().expect("hash");
    let third = segment(&key, 2, vec![record(b"three", false)], Some(second_hash));
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("first segment");
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &third.wire_bytes().expect("wire"),
            13,
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::SequenceGap {
                expected: 1,
                actual: 2,
            }
        ))
    ));
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &second.wire_bytes().expect("wire"),
            12,
        )
        .expect("repair the missing segment");
    let receipt = runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &third.wire_bytes().expect("wire"),
            13,
        )
        .expect("restore continuous acknowledgement");
    assert_eq!(receipt.acknowledgement.sequence, 2);
    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(11, 11, 10).expect("query"),
            LocalQueryMetadata::current_window(13),
        )
        .expect("query response");
    assert_eq!(response.plan.completeness(), Completeness::Complete);
    assert!(response.plan.gaps().is_empty());
}

#[test]
fn tombstones_and_unknown_schemas_are_preserved_without_resurrection_or_querying() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let tombstone = segment(&key, 0, vec![record(b"dead", true)], None);
    let tombstone_hash = tombstone.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &tombstone.wire_bytes().expect("wire"),
            11,
        )
        .expect("tombstone");
    let resurrection = segment(&key, 1, vec![record(b"dead", false)], Some(tombstone_hash));
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &resurrection.wire_bytes().expect("wire"),
            12,
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::ResurrectionPrevented
        ))
    ));
    let future = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 8, "runtime", 0).expect("cursor"),
        vec![SyncRecord::new(
            "subject-a",
            "node-a",
            "future.v99",
            99,
            b"future".to_vec(),
            b"raw".to_vec(),
            false,
        )],
        None,
        12,
        13,
    )
    .expect("future segment")
    .sign(&key)
    .expect("signature");
    let unknown_temporary = tempfile::tempdir().expect("temporary directory");
    let mut unknown_runtime = load(unknown_temporary.path());
    let receipt = unknown_runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &future.wire_bytes().expect("wire"),
            13,
        )
        .expect("unknown schema accepted");
    assert_eq!(receipt.unknown_schema_records, 1);
    let response = unknown_runtime
        .query(
            "repository-a",
            HistoryQuery::new(13, 13, 10).expect("query"),
            LocalQueryMetadata::current_window(13),
        )
        .expect("query response");
    assert_eq!(response.plan.completeness(), Completeness::LocalOnly);
    assert!(response.plan.coverage().is_some());
    assert!(!response.plan.gaps().is_empty());
    assert!(response.records.is_empty());
}

#[test]
fn expired_tombstones_allow_replacement_after_fresh_activity() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let tombstone = segment(&key, 0, vec![record(b"dead", true)], None);
    let tombstone_hash = tombstone.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &tombstone.wire_bytes().expect("wire"),
            100,
        )
        .expect("tombstone");
    let keep_fresh_at = 100 + TOMBSTONE_HORIZON_SECONDS - 1;
    let keep_fresh = segment(&key, 1, vec![record(b"other", false)], Some(tombstone_hash));
    let keep_fresh_hash = keep_fresh.segment_hash().expect("hash");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &keep_fresh.wire_bytes().expect("wire"),
            keep_fresh_at,
        )
        .expect("fresh activity");
    let replacement = segment(&key, 2, vec![record(b"dead", false)], Some(keep_fresh_hash));
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &replacement.wire_bytes().expect("wire"),
            keep_fresh_at + 1,
        )
        .expect("expired tombstone does not block a replacement record");
}

#[test]
fn low_space_stops_history_writes_but_not_control_plane_operations() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .force_capacity_for_test(0, HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES - 1)
        .expect("capacity");
    assert_eq!(
        runtime.history_write_availability(),
        HistoryWriteAvailability::DegradedLowSpace
    );
    assert!(runtime.control_plane_permitted());
    let history = segment(&key, 0, vec![record(b"blocked", false)], None);
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &history.wire_bytes().expect("wire"),
            11,
        ),
        Err(RepositoryRuntimeError::WriteStopped(
            HistoryWriteAvailability::DegradedLowSpace
        ))
    ));
}

#[test]
fn persisted_fork_quarantine_and_stale_replica_rebuild_fail_closed() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("first segment");
    let fork = segment(&key, 0, vec![record(b"fork", false)], None);
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &fork.wire_bytes().expect("wire"),
            12
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::ForkDetected { next_epoch: 8 }
        ))
    ));
    let mut restored = load(temporary.path());
    assert!(matches!(
        restored.receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            13
        ),
        Err(RepositoryRuntimeError::Protocol(ProtocolError::Quarantined))
    ));
    let rebuild_at = 11 + TOMBSTONE_HORIZON_SECONDS + 1;
    let rebuilt = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 0).expect("cursor"),
        vec![record(b"rebuilt", false)],
        None,
        rebuild_at,
        rebuild_at,
    )
    .expect("rebuilt segment")
    .sign(&key)
    .expect("signature");
    restored
        .receive_wire(
            "cluster-a",
            &identity,
            &rebuilt.wire_bytes().expect("wire"),
            rebuild_at,
        )
        .expect("stale replica rebuilds before accepting replacement stream");
    let response = restored
        .query(
            "repository-a",
            HistoryQuery::new(rebuild_at, rebuild_at, 10).expect("query"),
            LocalQueryMetadata::current_window(rebuild_at),
        )
        .expect("query response");
    assert_eq!(response.records.len(), 1);
}
