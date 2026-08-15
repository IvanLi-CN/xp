use super::*;
use crate::state::history_repository::identity::{
    Ed25519PublicKey, RepositoryNodeId, X25519PublicKey,
};

#[test]
fn repository_summary_cursor_accepts_phase_and_legacy_segment_ids() {
    let segment_id = "0123456789abcdef".repeat(4);
    assert!(valid_repository_summary_cursor(&segment_id));
    assert!(valid_repository_summary_cursor(&format!("t:{segment_id}")));
    assert!(valid_repository_summary_cursor(&format!("r:{segment_id}")));
    assert!(!valid_repository_summary_cursor(&format!("x:{segment_id}")));
    assert!(!valid_repository_summary_cursor("t:not-a-segment-id"));
}

#[test]
fn repository_segment_identity_must_match_authenticated_sender() {
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("repository-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes([1; 32]).expect("signing key"),
        X25519PublicKey::from_bytes([2; 32]).expect("relay key"),
    )
    .expect("identity");
    assert!(repository_identity_matches_sender(
        &identity,
        "repository-a"
    ));
    assert!(!repository_identity_matches_sender(
        &identity,
        "repository-b"
    ));
}

#[test]
fn relay_frame_must_match_its_claimed_repository_source() {
    let source = crate::history_sync::RelayKeypair::from_private_key([1; 32]);
    let other_source = crate::history_sync::RelayKeypair::from_private_key([2; 32]);
    let target = crate::history_sync::RelayKeypair::from_private_key([3; 32]);
    let frame = crate::history_sync::RelayFrame::seal(
        source,
        target.public_key(),
        [4; 12],
        b"repair batch",
        b"repository-c",
    )
    .expect("relay frame");
    assert!(relay_frame_matches_source(&frame, source));
    assert!(!relay_frame_matches_source(&frame, other_source));
}

#[test]
fn sync_wire_uses_identity_below_threshold_and_zstd_only_when_smaller() {
    let small = vec![7_u8; 4 * 1024 - 1];
    let (payload_encoding, encoded) =
        crate::history_sync::encode_payload(small.clone()).expect("small wire");
    let encoding = RepositorySyncWireEncoding::from(payload_encoding);
    assert_eq!(encoding, RepositorySyncWireEncoding::Identity);
    assert_eq!(encoded, small);

    let compressible = vec![0_u8; 4 * 1024 * 2];
    let (payload_encoding, encoded) =
        crate::history_sync::encode_payload(compressible.clone()).expect("compressible wire");
    let encoding = RepositorySyncWireEncoding::from(payload_encoding);
    assert_eq!(encoding, RepositorySyncWireEncoding::Zstd);
    assert!(encoded.len() < compressible.len());
    assert_eq!(
        crate::history_sync::decode_payload(payload_encoding, &encoded, compressible.len())
            .expect("bounded decode"),
        compressible
    );

    let mut state = 0x9e37_79b9_u32;
    let incompressible = (0..4 * 1024 * 2)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            u8::try_from(state & 0xff).expect("byte")
        })
        .collect::<Vec<_>>();
    let (payload_encoding, _) =
        crate::history_sync::encode_payload(incompressible).expect("raw wire");
    let encoding = RepositorySyncWireEncoding::from(payload_encoding);
    assert_eq!(encoding, RepositorySyncWireEncoding::Identity);
}

#[test]
fn sync_wire_allows_bounded_canonical_expansion_and_rejects_over_one_mib() {
    let mut state = 0x9e37_79b9_u32;
    let block = (0..1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            u8::try_from(state & 0xff).expect("byte")
        })
        .collect::<Vec<_>>();
    let expanded = block.repeat(300);
    assert!(expanded.len() > MAX_RESPONSE_WIRE_BYTES);
    let compressed = zstd::stream::encode_all(std::io::Cursor::new(&expanded), 1)
        .expect("compressed canonical payload");
    assert!(compressed.len() <= MAX_RESPONSE_WIRE_BYTES);
    assert_eq!(
        crate::history_sync::decode_payload(
            PayloadEncoding::ZstandardLevel1,
            &compressed,
            expanded.len(),
        )
        .expect("bounded canonical decode"),
        expanded
    );

    let over_limit = block.repeat(1025);
    let over_limit = zstd::stream::encode_all(std::io::Cursor::new(&over_limit), 1)
        .expect("compressed test payload");
    assert!(
        crate::history_sync::decode_payload(
            PayloadEncoding::ZstandardLevel1,
            &over_limit,
            1024 * 1024 + 1,
        )
        .is_err()
    );
}
