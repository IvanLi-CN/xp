use super::RepositoryRepairBatch;

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
