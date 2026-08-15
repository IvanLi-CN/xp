use ed25519_dalek::SigningKey;

use crate::state::history_repository::identity::{
    Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey,
};

use super::{
    Acceptance, CanonicalSegment, Cursor, CursorAvailability, ProtocolError, SchemaCatalog,
    SegmentReceiver, SyncRecord, cursor_availability,
};
use super::{
    DirectPath, DirectPathHealth, DynamicRelay, EncodedResponse, PathDecision, PathSelector,
    PayloadEncoding, RelayAttemptState, RelayFrame, RelayKeypair, prioritize_tombstones,
};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

fn cursor(sequence: u64) -> Cursor {
    Cursor::new("node-a", 7, "runtime", sequence).expect("valid cursor")
}

#[test]
fn cursor_rejects_values_outside_the_durable_sqlite_range() {
    assert!(Cursor::new("node-a", i64::MAX as u64 + 1, "runtime", 0).is_err());
    assert!(Cursor::new("node-a", 7, "runtime", i64::MAX as u64 + 1).is_err());
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

fn receiver(schemas: SchemaCatalog) -> SegmentReceiver {
    SegmentReceiver::for_cluster("cluster-a", schemas)
}

fn identity(key: &SigningKey) -> RepositoryNodeIdentity {
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("valid source node"),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).expect("valid signing key"),
        X25519PublicKey::from_bytes([9; 32]).expect("valid relay key"),
    )
    .expect("source identity")
}

fn signed_segment(
    key: &SigningKey,
    sequence: u64,
    records: Vec<SyncRecord>,
    previous_hash: Option<[u8; 32]>,
) -> super::SignedSegment {
    CanonicalSegment::new(
        "cluster-a",
        cursor(sequence),
        records,
        previous_hash,
        10,
        11,
    )
    .expect("bounded segment")
    .sign(key)
    .expect("source signature")
}

#[test]
fn canonical_segments_encode_identically_for_the_same_input() {
    let records = vec![record(b"node-a", false)];
    let cursor = cursor(42);

    let first = CanonicalSegment::new("cluster-a", cursor.clone(), records.clone(), None, 10, 11)
        .expect("valid segment");
    let second =
        CanonicalSegment::new("cluster-a", cursor, records, None, 10, 11).expect("valid segment");

    assert_eq!(
        first.canonical_bytes().expect("canonical bytes"),
        second.canonical_bytes().expect("canonical bytes")
    );
}

#[test]
fn tombstone_in_a_segment_prevents_same_segment_resurrection() {
    let key = signing_key();
    let segment = CanonicalSegment::new(
        "cluster-a",
        cursor(0),
        vec![record(b"subject-a", true), record(b"subject-a", false)],
        None,
        10,
        11,
    )
    .expect("segment is bounded")
    .sign(&key)
    .expect("source signs segment");
    let mut receiver = receiver(SchemaCatalog::new([("runtime.v1".to_owned(), 1)]));

    assert_eq!(
        receiver.accept(&segment, &identity(&key)),
        Err(ProtocolError::ResurrectionPrevented)
    );
}

#[test]
fn source_prioritizes_tombstones_and_receiver_rejects_unprioritized_segments() {
    let sorted = prioritize_tombstones(vec![record(b"ordinary", false), record(b"dead", true)]);
    assert!(sorted[0].is_tombstone());
    assert!(!sorted[1].is_tombstone());
    assert!(matches!(
        CanonicalSegment::new(
            "cluster-a",
            cursor(0),
            vec![record(b"ordinary", false), record(b"dead", true)],
            None,
            10,
            11,
        ),
        Err(ProtocolError::InvalidSegment(
            "tombstones must precede ordinary records"
        ))
    ));
}

#[test]
fn tombstones_do_not_block_an_independent_stream_with_the_same_record_key() {
    let key = signing_key();
    let identity = identity(&key);
    let mut receiver = receiver(SchemaCatalog::default());
    let tombstone = signed_segment(&key, 0, vec![record(b"shared-key", true)], None);
    receiver
        .accept(&tombstone, &identity)
        .expect("tombstone accepted");
    let other_stream = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 0).unwrap(),
        vec![record(b"shared-key", false)],
        None,
        10,
        11,
    )
    .unwrap()
    .sign(&key)
    .unwrap();

    assert!(receiver.accept(&other_stream, &identity).is_ok());
}

#[test]
fn receiver_rejects_cursor_gaps_without_advancing_acknowledgement() {
    let key = signing_key();
    let identity = identity(&key);
    let mut receiver = receiver(SchemaCatalog::new([("runtime.v1".to_owned(), 1)]));
    let initial = signed_segment(&key, 10, vec![record(b"a", false)], None);
    let accepted = receiver.accept(&initial, &identity).expect("first segment");
    assert_eq!(
        accepted.acknowledgement().watermark(),
        initial.canonical().last_cursor()
    );

    let gap = signed_segment(
        &key,
        12,
        vec![record(b"b", false)],
        Some(initial.segment_hash().unwrap()),
    );
    assert_eq!(
        receiver.accept(&gap, &identity),
        Err(ProtocolError::SequenceGap {
            expected: 11,
            actual: 12,
        })
    );
    assert_eq!(
        receiver.continuous_watermark(initial.canonical().first_cursor()),
        Some(initial.canonical().last_cursor().clone())
    );
}

#[test]
fn receiver_rotates_to_a_new_epoch_and_reports_a_gap() {
    let key = signing_key();
    let identity = identity(&key);
    let mut receiver = receiver(SchemaCatalog::default());
    let initial = signed_segment(&key, 0, vec![record(b"a", false)], None);
    receiver.accept(&initial, &identity).expect("initial epoch");
    let next_epoch = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 8, "runtime", 0).unwrap(),
        vec![record(b"different", false)],
        None,
        10,
        11,
    )
    .unwrap()
    .sign(&key)
    .unwrap();

    let accepted = receiver.accept(&next_epoch, &identity).unwrap();
    assert!(matches!(
        accepted,
        Acceptance::Accepted { gap: Some(_), .. }
    ));
    let gap = accepted.gap().expect("new epoch must report a gap");
    assert_eq!(gap.requested(), initial.canonical().last_cursor());
    assert_eq!(
        gap.earliest_available(),
        next_epoch.canonical().first_cursor()
    );
    assert!(!receiver.is_quarantined(initial.canonical().first_cursor()));
    assert_eq!(
        receiver.continuous_watermark(next_epoch.canonical().first_cursor()),
        Some(next_epoch.canonical().last_cursor().clone())
    );
}

#[test]
fn receiver_verifies_signatures_hash_chain_and_isolates_valid_forks() {
    let key = signing_key();
    let other_key = SigningKey::from_bytes(&[8; 32]);
    let identity = identity(&key);
    let mut receiver = receiver(SchemaCatalog::default());
    let first = signed_segment(&key, 0, vec![record(b"a", false)], None);
    receiver.accept(&first, &identity).expect("first segment");

    let forged = signed_segment(
        &other_key,
        1,
        vec![record(b"b", false)],
        Some(first.segment_hash().unwrap()),
    );
    assert_eq!(
        receiver.accept(&forged, &identity),
        Err(ProtocolError::InvalidSignature)
    );
    let wrong_chain = signed_segment(&key, 1, vec![record(b"b", false)], Some([0; 32]));
    assert_eq!(
        receiver.accept(&wrong_chain, &identity),
        Err(ProtocolError::HashChainMismatch)
    );
    let valid_fork = signed_segment(&key, 0, vec![record(b"different", false)], None);
    assert_eq!(
        receiver.accept(&valid_fork, &identity),
        Err(ProtocolError::ForkDetected { next_epoch: 8 })
    );
    assert!(receiver.is_quarantined(valid_fork.canonical().first_cursor()));
    assert_eq!(
        receiver.accept(&first, &identity),
        Err(ProtocolError::Quarantined)
    );
    let replacement = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 8, "runtime", 0).unwrap(),
        vec![record(b"replacement", false)],
        None,
        10,
        11,
    )
    .unwrap()
    .sign(&key)
    .unwrap();
    assert!(receiver.accept(&replacement, &identity).is_ok());
    let continued = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 8, "runtime", 1).unwrap(),
        vec![record(b"continued", false)],
        Some(replacement.segment_hash().unwrap()),
        10,
        11,
    )
    .unwrap()
    .sign(&key)
    .unwrap();
    assert!(receiver.accept(&continued, &identity).is_ok());
}

#[test]
fn receiver_rejects_a_valid_segment_from_another_cluster() {
    let key = signing_key();
    let identity = identity(&key);
    let segment = signed_segment(&key, 0, vec![record(b"a", false)], None);
    let mut receiver = SegmentReceiver::for_cluster("cluster-b", SchemaCatalog::default());

    assert_eq!(
        receiver.accept(&segment, &identity),
        Err(ProtocolError::ClusterMismatch)
    );
}

#[test]
fn unknown_schemas_are_accepted_without_interpretation() {
    let key = signing_key();
    let identity = identity(&key);
    let segment = signed_segment(
        &key,
        0,
        vec![SyncRecord::new(
            "subject-a",
            "node-a",
            "future.v99",
            99,
            b"a".to_vec(),
            b"raw".to_vec(),
            false,
        )],
        None,
    );
    let mut receiver = receiver(SchemaCatalog::default());

    assert_eq!(
        receiver
            .accept(&segment, &identity)
            .unwrap()
            .unknown_schema_records(),
        1
    );
    assert_eq!(receiver.forwardable_unknown_segments(), &[segment]);
}

#[test]
fn unknown_schema_forwarding_uses_a_bounded_in_memory_queue() {
    let key = signing_key();
    let identity = identity(&key);
    let mut receiver = receiver(SchemaCatalog::default());
    let mut previous = None;

    for sequence in 0..5 {
        let segment = signed_segment(
            &key,
            sequence,
            vec![SyncRecord::new(
                "subject-a",
                "node-a",
                "future.v99",
                99,
                format!("key-{sequence}").into_bytes(),
                b"raw".to_vec(),
                false,
            )],
            previous,
        );
        previous = Some(segment.segment_hash().unwrap());
        receiver.accept(&segment, &identity).unwrap();
    }

    let overflow = signed_segment(
        &key,
        5,
        vec![SyncRecord::new(
            "subject-a",
            "node-a",
            "future.v99",
            99,
            b"key-overflow".to_vec(),
            b"raw".to_vec(),
            false,
        )],
        previous,
    );
    assert_eq!(
        receiver.accept(&overflow, &identity),
        Err(ProtocolError::UnknownSchemaBacklogFull)
    );
    assert_eq!(receiver.forwardable_unknown_segments().len(), 5);
}

#[test]
fn segment_bounds_and_canonical_wire_are_enforced_before_acceptance() {
    let key = signing_key();
    let many_records = (0..1_001)
        .map(|index| record(format!("key-{index}").as_bytes(), false))
        .collect();
    assert!(matches!(
        CanonicalSegment::new("cluster-a", cursor(0), many_records, None, 10, 11),
        Err(ProtocolError::SegmentRecordLimit { actual: 1_001 })
    ));
    assert!(matches!(
        CanonicalSegment::new(
            "cluster-a",
            cursor(0),
            vec![record(b"a", false)],
            None,
            10,
            71
        ),
        Err(ProtocolError::SegmentDurationLimit)
    ));

    let signed = signed_segment(&key, 0, vec![record(b"a", false)], None);
    let wire = signed.wire_bytes().unwrap();
    assert_eq!(super::SignedSegment::from_wire(&wire).unwrap(), signed);
    let mut noncanonical = wire;
    noncanonical.extend_from_slice(&[0xa0, 0x01, 0x00]);
    assert_eq!(
        super::SignedSegment::from_wire(&noncanonical),
        Err(ProtocolError::NonCanonicalWire)
    );
}

#[test]
fn response_encoding_uses_identity_or_bounded_zstandard_and_rejects_bombs() {
    let small = EncodedResponse::encode(vec![7; 4_095]).unwrap();
    assert_eq!(small.encoding(), PayloadEncoding::Identity);
    assert_eq!(small.decode().unwrap(), vec![7; 4_095]);

    let compressible = vec![b'x'; 32 * 1024];
    let compressed = EncodedResponse::encode(compressible.clone()).unwrap();
    assert_eq!(compressed.encoding(), PayloadEncoding::ZstandardLevel1);
    assert_eq!(compressed.decode().unwrap(), compressible);
    let incompressible = deterministic_noise(16 * 1024);
    let identity = EncodedResponse::encode(incompressible).unwrap();
    assert_eq!(identity.encoding(), PayloadEncoding::Identity);

    let bomb_wire =
        zstd::stream::encode_all(std::io::Cursor::new(vec![0; 1024 * 1024]), 1).unwrap();
    assert_eq!(
        EncodedResponse::from_wire(PayloadEncoding::ZstandardLevel1, 1024 * 1024, bomb_wire),
        Err(ProtocolError::CompressionExpansionLimit)
    );
    assert!(matches!(
        EncodedResponse::from_wire(PayloadEncoding::Identity, 1, vec![0; 256 * 1024 + 1]),
        Err(ProtocolError::WireLimit { .. })
    ));
    let small_compressed = zstd::stream::encode_all(std::io::Cursor::new(vec![0; 128]), 1).unwrap();
    assert_eq!(
        EncodedResponse::from_wire(PayloadEncoding::ZstandardLevel1, 128, small_compressed),
        Err(ProtocolError::EncodingDecision)
    );
    assert_eq!(
        EncodedResponse::from_wire(PayloadEncoding::Identity, 8 * 1024, vec![0; 8 * 1024]),
        Err(ProtocolError::EncodingDecision)
    );
}

#[test]
fn record_identity_and_expired_cursor_gaps_are_preserved() {
    let record = record(b"key", false);
    assert_eq!(record.subject_node_id(), "subject-a");
    assert_eq!(record.observer_node_id(), "node-a");

    let requested = cursor(4);
    let earliest = cursor(8);
    let availability = cursor_availability(requested.clone(), earliest.clone()).unwrap();
    let CursorAvailability::Expired(gap) = availability else {
        panic!("expired cursor must produce an explicit gap");
    };
    assert_eq!(gap.requested(), &requested);
    assert_eq!(gap.earliest_available(), &earliest);

    let prior_epoch = Cursor::new("node-a", 6, "runtime", 100).unwrap();
    let CursorAvailability::Expired(gap) =
        cursor_availability(prior_epoch.clone(), earliest.clone())
            .expect("a prior epoch is expired")
    else {
        panic!("a prior epoch must produce a gap");
    };
    assert_eq!(gap.requested(), &prior_epoch);
    assert_eq!(gap.earliest_available(), &earliest);
}

fn deterministic_noise(len: usize) -> Vec<u8> {
    let mut state = 0x6a09_e667_f3bc_c909_u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

#[test]
fn peer_direct_paths_are_equal_and_relay_is_hourly_after_both_fail() {
    let healthy_reality = DirectPathHealth {
        healthy: true,
        stable_for_seconds: 100,
    };
    let healthy_tunnel = DirectPathHealth {
        healthy: true,
        stable_for_seconds: 200,
    };
    let unhealthy = DirectPathHealth {
        healthy: false,
        stable_for_seconds: 0,
    };
    let relay = RelayAttemptState::new(Some(1_000), 30, true);
    let mut selector = PathSelector::default();

    assert_eq!(
        selector.select(healthy_reality, healthy_tunnel, relay, 1_500),
        PathDecision::Direct {
            path: DirectPath::CloudflareTunnel,
            probe_standby: true,
        }
    );
    let nearly_as_stable_reality = DirectPathHealth {
        healthy: true,
        stable_for_seconds: 180,
    };
    assert_eq!(
        selector.select(nearly_as_stable_reality, healthy_tunnel, relay, 1_501),
        PathDecision::Direct {
            path: DirectPath::CloudflareTunnel,
            probe_standby: false,
        }
    );
    assert_eq!(
        selector.select(unhealthy, unhealthy, relay, 4_629),
        PathDecision::Unavailable {
            next_relay_attempt_unix_seconds: Some(4_630),
        }
    );
    assert_eq!(
        selector.select(unhealthy, unhealthy, relay, 4_630),
        PathDecision::DynamicRelay
    );
    assert_eq!(
        selector.select(
            unhealthy,
            unhealthy,
            RelayAttemptState::new(None, 30, true),
            4_000,
        ),
        PathDecision::DynamicRelay
    );
}

#[test]
fn relay_keeps_payload_end_to_end_and_counts_only_control_bytes() {
    let sender = RelayKeypair::from_private_key([1; 32]);
    let receiver = RelayKeypair::from_private_key([2; 32]);
    let mut frame = RelayFrame::seal(
        sender,
        receiver.public_key(),
        [3; 12],
        b"history",
        b"header",
    )
    .expect("seal payload");
    let mut relay = DynamicRelay::default();
    let receipt = relay.forward(&frame).expect("stream-only forward");
    assert!(receipt.sync_control_bytes().count() > 0);
    assert_eq!(
        relay.forwarded_sync_control_bytes(),
        receipt.sync_control_bytes()
    );
    assert_eq!(
        receipt.relay_bytes().count(),
        receipt.sync_control_bytes().count()
    );
    assert_eq!(frame.open(receiver, b"header").unwrap(), b"history");
    frame.tamper_for_test();
    assert!(frame.open(receiver, b"header").is_err());
}

#[test]
fn relay_enforces_the_total_wire_limit_including_encryption_overhead() {
    let sender = RelayKeypair::from_private_key([1; 32]);
    let receiver = RelayKeypair::from_private_key([2; 32]);
    let maximum = vec![0; 256 * 1024 - 32 - 12 - 16];
    let frame = RelayFrame::seal(sender, receiver.public_key(), [3; 12], &maximum, b"").unwrap();
    assert_eq!(frame.wire_len_for_test(), 256 * 1024);
    assert_eq!(
        RelayFrame::seal(
            sender,
            receiver.public_key(),
            [3; 12],
            &vec![0; 256 * 1024 - 32 - 12 - 16 + 1],
            b"",
        ),
        Err(super::RelayError::FrameLimit)
    );
}
