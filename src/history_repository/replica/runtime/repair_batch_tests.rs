use super::{
    RepositoryRepairBatch,
    tests::{identity, load, record, segment, signing_key},
};

#[test]
fn repair_response_digest_binds_the_returned_content() {
    let mut response = RepositoryRepairBatch {
        segments: Vec::new(),
        unavailable_segment_ids: Vec::new(),
        gaps: Vec::new(),
        history_truncated: false,
        response_id: None,
    };
    let original = response.response_id_digest().expect("response digest");

    response.history_truncated = true;
    assert_ne!(
        original,
        response
            .response_id_digest()
            .expect("changed response digest")
    );
}

#[test]
fn repair_response_id_accepts_only_the_same_returned_content() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = signing_key();
    let identity = identity(&signing_key);
    let segment = segment(&signing_key, 0, vec![record(b"repair", false)], None);
    let wire = segment.wire_bytes().expect("segment wire");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire("cluster-a", &identity, &wire, 12)
        .expect("store repair segment");
    let segment_id = runtime
        .replication_summary()
        .expect("summary")
        .segment_ids
        .pop()
        .expect("segment id");

    let response = runtime
        .repair_batch(std::slice::from_ref(&segment_id))
        .expect("repair response");
    let response_id = response.response_id.clone().expect("response id");
    assert_eq!(
        response.response_id_digest().expect("response digest"),
        response_id
    );

    let mut legacy_response = response.clone();
    legacy_response.response_id = None;
    assert_eq!(
        legacy_response.response_id_digest().expect("legacy digest"),
        response_id
    );
    runtime
        .repair_batch_with_response_id(std::slice::from_ref(&segment_id), Some(&response_id))
        .expect("same response accepted");
    assert!(
        runtime
            .repair_batch_with_response_id(&[segment_id], Some("changed-response"))
            .is_err()
    );
}
