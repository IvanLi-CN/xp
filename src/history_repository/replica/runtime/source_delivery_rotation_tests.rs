use super::*;

#[test]
fn source_delivery_replay_page_rotates_before_exhausting_a_hot_stream() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    for sequence in 0..256_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "connections.v1",
                    1,
                    format!("connection:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue hot stream backlog");
    }
    runtime
        .queue_local_source_segment(
            "cluster-a",
            source_identity,
            &signing_key,
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                b"runtime:0".to_vec(),
                b"sample".to_vec(),
                false,
            )],
            256,
        )
        .expect("queue later stream backlog");

    let page = runtime.local_source_pending_segments_page();
    let streams = page
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("signed replay segment")
                .canonical()
                .first_cursor()
                .stream()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        streams.contains(&"runtime".to_owned()),
        "a later stream head must not be starved by the hot stream"
    );
}
