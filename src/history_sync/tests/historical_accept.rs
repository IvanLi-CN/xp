use super::*;

#[test]
fn historical_accept_requires_each_deleted_record_without_changing_wire_or_live_policy() {
    let key = signing_key();
    let source = identity(&key);
    let mut receiver = receiver(SchemaCatalog::new([("runtime.v1".to_owned(), 1)]));
    let deleted = signed_segment(
        &key,
        0,
        vec![record(b"deleted-a", true), record(b"deleted-b", true)],
        None,
    );
    receiver.accept(&deleted, &source).unwrap();
    let historical = signed_segment(
        &key,
        2,
        vec![
            record(b"deleted-a", false),
            record(b"retained", false),
            record(b"deleted-b", false),
            record(b"deleted-a", false),
        ],
        Some(deleted.segment_hash().unwrap()),
    );
    let wire = historical.wire_bytes().unwrap();
    let before = serde_json::to_value(receiver.checkpoint().unwrap()).unwrap();
    assert_eq!(
        receiver.accept_historical(&historical, &source, |cursor, _| cursor.sequence() == 2),
        Err(ProtocolError::ResurrectionPrevented)
    );
    assert_eq!(
        serde_json::to_value(receiver.checkpoint().unwrap()).unwrap(),
        before
    );
    assert_eq!(
        receiver.accept_historical(&historical, &source, |position, _| {
            matches!(position.sequence(), 2 | 4)
        }),
        Err(ProtocolError::ResurrectionPrevented),
        "approval of one occurrence does not approve a later record with the same key"
    );
    let accepted = receiver
        .accept_historical(&historical, &source, |position, record| {
            position.source_node_id() == "node-a"
                && position.source_epoch() == 7
                && position.stream() == "runtime"
                && matches!(
                    (position.sequence(), record.record_key()),
                    (2 | 5, b"deleted-a") | (4, b"deleted-b")
                )
        })
        .expect("every conflicting record has its own trusted deletion decision");
    assert_eq!(accepted.acknowledgement().watermark(), &cursor(5));
    assert_eq!(historical.wire_bytes().unwrap(), wire);
    historical.verify_identity(&source).unwrap();
    let replay = super::super::SignedSegment::from_wire(&wire).unwrap();
    assert!(matches!(
        receiver.accept(&replay, &source),
        Ok(Acceptance::Duplicate { .. })
    ));
    let mut receiver = SegmentReceiver::from_checkpoint(
        "cluster-a",
        SchemaCatalog::new([("runtime.v1".to_owned(), 1)]),
        receiver.checkpoint().unwrap(),
    )
    .unwrap();
    let live = signed_segment(
        &key,
        6,
        vec![record(b"deleted-a", false)],
        Some(historical.segment_hash().unwrap()),
    );
    assert_eq!(
        receiver.accept(&live, &source),
        Err(ProtocolError::ResurrectionPrevented)
    );
    assert_eq!(
        receiver.continuous_watermark(&cursor(0)).unwrap(),
        Some(cursor(5))
    );
}

#[test]
fn historical_accept_requires_deletion_decision_for_same_segment_conflict() {
    let key = signing_key();
    let source = identity(&key);
    for prior_tombstone in [false, true] {
        let mut receiver = receiver(SchemaCatalog::default());
        let seed = signed_segment(&key, 0, vec![record(b"deleted", true)], None);
        let (sequence, previous) = if prior_tombstone {
            receiver.accept(&seed, &source).unwrap();
            (1, Some(seed.segment_hash().unwrap()))
        } else {
            (0, None)
        };
        let conflict = signed_segment(
            &key,
            sequence,
            vec![record(b"deleted", true), record(b"deleted", false)],
            previous,
        );
        let before = serde_json::to_value(receiver.checkpoint().unwrap()).unwrap();
        assert_eq!(
            receiver.accept_historical(&conflict, &source, |_, _| false),
            Err(ProtocolError::ResurrectionPrevented)
        );
        assert_eq!(
            serde_json::to_value(receiver.checkpoint().unwrap()).unwrap(),
            before
        );
        receiver
            .accept_historical(&conflict, &source, |_, _| true)
            .unwrap();
    }
}

#[test]
fn historical_accept_checks_authentication_and_chain_before_deletion_decisions() {
    let key = signing_key();
    let source = identity(&key);
    let mut receiver = receiver(SchemaCatalog::new([("runtime.v1".to_owned(), 1)]));
    let seed = signed_segment(&key, 0, vec![record(b"deleted", true)], None);
    receiver.accept(&seed, &source).unwrap();
    let previous = Some(seed.segment_hash().unwrap());
    let build = |cluster: &str, position: Cursor, previous| {
        CanonicalSegment::new(
            cluster,
            position,
            vec![record(b"deleted", false)],
            previous,
            10,
            11,
        )
        .unwrap()
        .sign(&key)
        .unwrap()
    };
    let other_key = SigningKey::from_bytes(&[8; 32]);
    let cases = [
        (
            signed_segment(&other_key, 1, vec![record(b"deleted", false)], previous),
            ProtocolError::InvalidSignature,
        ),
        (
            build(
                "cluster-a",
                Cursor::new("node-b", 7, "runtime", 1).unwrap(),
                previous,
            ),
            ProtocolError::SourceIdentityMismatch,
        ),
        (
            build("cluster-b", cursor(1), previous),
            ProtocolError::ClusterMismatch,
        ),
        (
            build("cluster-a", cursor(2), previous),
            ProtocolError::SequenceGap {
                expected: 1,
                actual: 2,
            },
        ),
        (
            build(
                "cluster-a",
                Cursor::new("node-a", 9, "runtime", 0).unwrap(),
                None,
            ),
            ProtocolError::EpochGap {
                expected: 8,
                actual: 9,
            },
        ),
        (
            build("cluster-a", cursor(1), Some([42; 32])),
            ProtocolError::HashChainMismatch,
        ),
        (
            build("cluster-a", cursor(1), None),
            ProtocolError::HashChainMismatch,
        ),
    ];
    let before = serde_json::to_value(receiver.checkpoint().unwrap()).unwrap();
    for (invalid, expected) in cases {
        assert_eq!(
            receiver.accept_historical(&invalid, &source, |_, _| {
                panic!("unverified input must not reach the trusted deletion predicate")
            }),
            Err(expected)
        );
        assert_eq!(
            serde_json::to_value(receiver.checkpoint().unwrap()).unwrap(),
            before
        );
    }
    // A valid retry can still receive and acknowledge the exact next signed segment.
    let valid = build("cluster-a", cursor(1), previous);
    let acceptance = receiver
        .accept_historical(&valid, &source, |_, _| true)
        .unwrap();
    assert_eq!(acceptance.acknowledgement().watermark(), &cursor(1));
}

#[test]
fn historical_accept_cannot_anchor_a_nonzero_first_sequence() {
    let key = signing_key();
    let source = identity(&key);
    let mut receiver = receiver(SchemaCatalog::default());
    let tail = signed_segment(&key, 2262, vec![record(b"deleted", false)], Some([42; 32]));
    assert_eq!(
        receiver.accept_historical(&tail, &source, |_, _| true),
        Err(ProtocolError::HashChainMismatch)
    );
    assert_eq!(receiver.continuous_watermark(&cursor(0)).unwrap(), None);
}

#[test]
fn historical_accept_keeps_fork_quarantine_across_checkpoint_restore() {
    let key = signing_key();
    let source = identity(&key);
    let mut receiver = receiver(SchemaCatalog::new([("runtime.v1".to_owned(), 1)]));
    let seed = signed_segment(&key, 0, vec![record(b"deleted", true)], None);
    receiver.accept(&seed, &source).unwrap();
    let previous = Some(seed.segment_hash().unwrap());
    let historical = signed_segment(&key, 1, vec![record(b"deleted", false)], previous);
    receiver
        .accept_historical(&historical, &source, |_, _| true)
        .unwrap();
    let fork = signed_segment(&key, 1, vec![record(b"changed", false)], previous);
    assert_eq!(
        receiver.accept_historical(&fork, &source, |_, _| true),
        Err(ProtocolError::ForkDetected { next_epoch: 8 })
    );
    let receiver = SegmentReceiver::from_checkpoint(
        "cluster-a",
        SchemaCatalog::new([("runtime.v1".to_owned(), 1)]),
        receiver.checkpoint().unwrap(),
    )
    .unwrap();
    assert!(receiver.is_quarantined(&cursor(0)));
    assert_eq!(
        receiver.continuous_watermark(&cursor(0)).unwrap(),
        Some(cursor(1))
    );
}

#[test]
fn historical_accept_keeps_all_unknown_signed_records_and_new_tombstones_after_restart() {
    let key = signing_key();
    let source = identity(&key);
    let mut receiver = receiver(SchemaCatalog::default());
    let seed = signed_segment(&key, 0, vec![record(b"deleted", true)], None);
    receiver.accept(&seed, &source).unwrap();
    let historical = signed_segment(
        &key,
        1,
        vec![
            record(b"new-tombstone", true),
            record(b"deleted", false),
            record(b"retained-unknown", false),
        ],
        Some(seed.segment_hash().unwrap()),
    );
    let wire = historical.wire_bytes().unwrap();
    let accepted = receiver
        .accept_historical(&historical, &source, |position, record| {
            position == &cursor(2) && record.record_key() == b"deleted"
        })
        .unwrap();
    assert_eq!(accepted.acknowledgement().watermark(), &cursor(3));
    assert_eq!(accepted.unknown_schema_records(), 3);
    let mut receiver = SegmentReceiver::from_checkpoint(
        "cluster-a",
        SchemaCatalog::default(),
        receiver.checkpoint().unwrap(),
    )
    .unwrap();
    let forwarded = receiver.forwardable_unknown_segments().last().unwrap();
    assert_eq!(forwarded.wire_bytes().unwrap(), wire);
    forwarded.verify_identity(&source).unwrap();
    assert_eq!(forwarded.canonical().records().len(), 3);
    let live = signed_segment(
        &key,
        4,
        vec![record(b"new-tombstone", false)],
        Some(historical.segment_hash().unwrap()),
    );
    assert_eq!(
        receiver.accept(&live, &source),
        Err(ProtocolError::ResurrectionPrevented)
    );
}
