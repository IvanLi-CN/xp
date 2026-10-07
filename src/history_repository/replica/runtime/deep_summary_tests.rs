use super::{identity, load, record, segment, signing_key};

#[test]
fn deep_verification_compares_retained_record_partitions() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let segment = segment(&key, 0, vec![record(b"retained", false)], None);
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment.wire_bytes().expect("wire"),
            11,
        )
        .expect("segment");
    let remote = runtime.replication_summary().expect("summary");
    let mut remote_json = serde_json::to_value(&remote).expect("summary JSON");
    remote_json["partitions"][0]["hash"][0] = serde_json::json!(255);
    let mut remote: super::super::sync::RepositoryReplicaSummary =
        serde_json::from_value(remote_json).expect("changed remote summary");
    remote.segment_ids.clear();
    remote.summary_version = 1;
    remote.sequence_blocks.clear();
    assert!(
        runtime
            .requires_repair(&remote, true)
            .expect("retained partition mismatch needs repair")
    );
    assert!(
        !runtime
            .retained_partitions_converged(&remote)
            .expect("retained partition mismatch is observable")
    );
    assert!(
        !runtime
            .requires_repair(&remote, false)
            .expect("shallow verification ignores retained partition summaries")
    );
}
