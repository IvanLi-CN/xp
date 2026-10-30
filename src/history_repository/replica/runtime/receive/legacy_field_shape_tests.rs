use super::*;

fn signed_records(
    stream: &str,
    schema: &str,
    start: u64,
    count: usize,
    previous: Option<[u8; 32]>,
) -> SignedSegment {
    let records = (0..count)
        .map(|offset| {
            let key = format!("{stream}-{}", start + offset as u64);
            SyncRecord::new(
                "subject-a",
                "node-a",
                schema,
                1,
                key.into_bytes(),
                b"sample".to_vec(),
                false,
            )
        })
        .collect();
    CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, stream, start).unwrap(),
        records,
        previous,
        11,
        11,
    )
    .unwrap()
    .sign(&signing_key())
    .unwrap()
}

fn journal_row(signed: &SignedSegment) -> SourceDeliveryJournalRow {
    let wire = signed.wire_bytes().unwrap();
    SourceDeliveryJournalRow {
        id: hex::encode(Sha256::digest(&wire)),
        stream: signed.canonical().first_cursor().stream().into(),
        closed_at_unix_seconds: 11,
        identity: identity(&signing_key()),
        wire,
    }
}

#[test]
fn legacy_field_shape_53_segments_three_streams_resume_and_exact_ack() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load(directory.path());
    let streams = [
        ("runtime", "runtime.v1", 2262_u64, 18_usize),
        ("path_health", "path_health.v1", 2262, 18),
        ("traffic", "traffic.v1", 3368, 17),
    ];
    let mut prefixes = Vec::new();
    let mut page_by_stream = Vec::new();
    let mut tails = BTreeMap::new();
    let mut page_index = 0_usize;
    for (stream, schema, head, segments) in streams {
        let mut sequence = 0;
        let mut hash = None;
        while sequence < head {
            let count = usize::try_from((head - sequence).min(64)).unwrap();
            let signed = signed_records(stream, schema, sequence, count, hash);
            hash = Some(signed.segment_hash().unwrap());
            sequence += count as u64;
            prefixes.push(signed);
        }
        let mut page = Vec::new();
        for _ in 0..segments {
            let count = if page_index < 22 { 3 } else { 2 };
            let signed = signed_records(stream, schema, sequence, count, hash);
            hash = Some(signed.segment_hash().unwrap());
            sequence += count as u64;
            page_index += 1;
            page.push(signed);
        }
        tails.insert(stream, (sequence, hash));
        page_by_stream.push(page);
    }
    let mut originals = Vec::new();
    for position in 0..18 {
        for stream in &page_by_stream {
            if let Some(segment) = stream.get(position) {
                originals.push(segment.clone());
            }
        }
    }
    assert_eq!(originals.len(), 53);
    assert_eq!(
        originals
            .iter()
            .map(|s| s.canonical().records().len())
            .sum::<usize>(),
        128
    );
    assert_eq!(originals[0].canonical().first_cursor().sequence(), 2262);
    let extra = signed_records("connections", "connections.v1", 0, 1, None);
    let mut journal = originals.iter().map(journal_row).collect::<Vec<_>>();
    let original_ids = journal.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let extra_row = journal_row(&extra);
    journal.push(extra_row.clone());
    runtime
        .storage
        .append_source_delivery_journal(&journal)
        .unwrap();
    let mut snapshot = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    snapshot["local_source"]["epoch"] = serde_json::json!(7);
    snapshot["local_source"]
        .as_object_mut()
        .unwrap()
        .remove("node_id");
    let mut source_streams = serde_json::Map::new();
    for (stream, (next, hash)) in &tails {
        source_streams.insert(
            (*stream).to_owned(),
            serde_json::json!({
                "next_sequence":next, "previous_segment_hash":hash, "pending":[]
            }),
        );
    }
    source_streams.insert(
        "connections".into(),
        serde_json::json!({
            "next_sequence":1, "previous_segment_hash":extra.segment_hash().unwrap(), "pending":[]
        }),
    );
    snapshot["local_source"]["streams"] = source_streams.into();
    snapshot["local_history_backfill_inflight"] = serde_json::json!({
        "segment_ids": original_ids, "page_cursor":null, "completed":true
    });
    runtime.snapshot = serde_json::from_value(snapshot).unwrap();
    runtime.persist_control_state().unwrap();

    let source = identity(&signing_key());
    let mut state = runtime
        .legacy_original_recovery_state("cluster-a", &source, 12)
        .unwrap();
    for (index, batch) in prefixes.chunks(16).enumerate() {
        let mut request = state.clone();
        request.pending = batch.iter().map(|signed| journal_row(signed).id).collect();
        runtime
            .legacy_recovery_update_scan(&state, &request)
            .unwrap();
        state = runtime
            .legacy_recovery_stage_response("cluster-a", &source, &request, &response(batch))
            .unwrap();
        if index % 4 == 0 {
            drop(runtime);
            runtime = load(directory.path());
            state = runtime
                .legacy_original_recovery_state("cluster-a", &source, 12)
                .unwrap();
        }
    }
    assert_eq!(state.phase, "apply");
    assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 54);
    let mut complete = false;
    for index in 0..100 {
        let state = runtime
            .legacy_original_recovery_state("cluster-a", &source, 12)
            .unwrap();
        complete = runtime
            .legacy_recovery_apply_step(
                "cluster-a",
                &source,
                &state,
                12,
                &["local".into()],
                "local",
            )
            .unwrap();
        if complete {
            break;
        }
        assert_eq!(runtime.storage.source_delivery_journal().unwrap().len(), 54);
        if index % 4 == 0 {
            drop(runtime);
            runtime = load(directory.path());
        }
    }
    assert!(complete);
    drop(runtime);
    runtime = load(directory.path());
    let remaining = runtime.storage.source_delivery_journal().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, extra_row.id);
    assert_eq!(remaining[0].wire, extra_row.wire);
    assert!(runtime.snapshot.local_history_backfill_completed);
    assert!(runtime.snapshot.local_history_backfill_inflight.is_none());
    for (stream, (next, hash)) in &tails {
        let cursor = Cursor::new("node-a", 7, *stream, 0).unwrap();
        assert_eq!(
            runtime
                .receiver
                .as_ref()
                .unwrap()
                .continuous_watermark(&cursor)
                .unwrap()
                .unwrap()
                .sequence(),
            next - 1
        );
        let checkpoint =
            serde_json::to_value(runtime.receiver.as_ref().unwrap().checkpoint().unwrap()).unwrap();
        let entry = checkpoint["streams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| {
                entry["key"]["source_node_id"] == "node-a" && entry["key"]["stream"] == *stream
            })
            .unwrap();
        assert_eq!(entry["progress"]["hash_chain_verified"], true);
        assert_eq!(
            entry["progress"]["last_segment_hash"],
            serde_json::json!(hash.unwrap())
        );
    }
    let expected = 2262 + 2262 + 3368 + 128;
    assert_eq!(
        runtime.storage.repository_history_record_count().unwrap(),
        expected
    );
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    let (count, unique): (u64, u64) = connection
        .query_row(
            concat!(
                "SELECT COUNT(*), COUNT(DISTINCT source_node_id || ':' || source_epoch ",
                "|| ':' || stream || ':' || sequence) FROM repository_history_records ",
                "WHERE is_tombstone=0"
            ),
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, u64::try_from(expected).unwrap());
    assert_eq!(count, unique);
}
