use super::*;

#[test]
fn verified_history_duplicate_requires_the_exact_current_tip_and_authentic_identity() {
    let key = signing_key();
    let source = identity(&key);
    let mut receiver = receiver(SchemaCatalog::default());
    let original = signed_segment(&key, 0, vec![record(b"old", false)], None);
    receiver.accept(&original, &source).unwrap();
    let next = signed_segment(
        &key,
        1,
        vec![record(b"new", false)],
        Some(original.segment_hash().unwrap()),
    );
    receiver.accept(&next, &source).unwrap();
    let before = serde_json::to_value(receiver.checkpoint().unwrap()).unwrap();
    for (tip, hash) in [
        (cursor(0), original.segment_hash().unwrap()),
        (cursor(1), [8; 32]),
        (
            Cursor::new("node-a", 8, "runtime", 1).unwrap(),
            next.segment_hash().unwrap(),
        ),
        (
            Cursor::new("node-a", 7, "other", 1).unwrap(),
            next.segment_hash().unwrap(),
        ),
    ] {
        assert_eq!(
            receiver.accept_verified_history_duplicate(&original, &source, &tip, hash),
            Err(ProtocolError::HashChainMismatch)
        );
    }
    let other = identity(&SigningKey::from_bytes(&[9; 32]));
    assert!(
        receiver
            .accept_verified_history_duplicate(
                &original,
                &other,
                &cursor(1),
                next.segment_hash().unwrap()
            )
            .is_err()
    );
    let receipt = receiver
        .accept_verified_history_duplicate(
            &original,
            &source,
            &cursor(1),
            next.segment_hash().unwrap(),
        )
        .unwrap();
    assert!(matches!(receipt, Acceptance::Duplicate { .. }));
    assert_eq!(receipt.acknowledgement().watermark(), &cursor(1));
    assert_eq!(
        serde_json::to_value(receiver.checkpoint().unwrap()).unwrap(),
        before
    );
    receiver = SegmentReceiver::from_checkpoint(
        "cluster-a",
        SchemaCatalog::default(),
        receiver.checkpoint().unwrap(),
    )
    .unwrap();
    assert_eq!(
        receiver.continuous_watermark_with_hash(&cursor(0)).unwrap(),
        Some((cursor(1), next.segment_hash().unwrap()))
    );
}

#[test]
fn retained_anchor_is_not_evidence_of_receipt_for_its_missing_prefix() {
    let key = signing_key();
    let source = identity(&key);
    let mut receiver = receiver(SchemaCatalog::default());
    let anchor = signed_segment(&key, 5, vec![record(b"retained", false)], Some([9; 32]));
    receiver.accept_retained_anchor(&anchor, &source).unwrap();
    let before = serde_json::to_value(receiver.checkpoint().unwrap()).unwrap();
    assert_eq!(
        receiver.continuous_watermark_with_hash(&cursor(0)),
        Err(ProtocolError::HashChainMismatch)
    );
    assert_eq!(
        receiver.accept_verified_history_duplicate(
            &anchor,
            &source,
            &cursor(5),
            anchor.segment_hash().unwrap()
        ),
        Err(ProtocolError::HashChainMismatch)
    );
    assert_eq!(
        serde_json::to_value(receiver.checkpoint().unwrap()).unwrap(),
        before
    );
}
