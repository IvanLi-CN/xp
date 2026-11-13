use super::prefix_recovery::*;
use super::tests::{identity, load, record, signing_key};
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer as _, SigningKey};

fn segment(
    key: &SigningKey,
    sequence: u64,
    records: Vec<SyncRecord>,
    previous: Option<[u8; 32]>,
) -> SignedSegment {
    CanonicalSegment::new(
        xp_test_fixtures::primary_cluster_id(),
        Cursor::new("node-a", 7, "runtime", sequence).unwrap(),
        records,
        previous,
        10,
        11,
    )
    .unwrap()
    .sign(key)
    .unwrap()
}

fn bundle(head: &SignedSegment, records: Vec<StoredRecord>) -> PrefixRecoveryBundle {
    let key = signing_key();
    let mut bundle = PrefixRecoveryBundle {
        version: 1,
        cluster_id: xp_test_fixtures::primary_cluster_id().into(),
        collector_node_id: "local".into(),
        identity: identity(&key),
        cluster_binding: "bound-membership".into(),
        source_deletion_version: 0,
        deletion_baseline_wire: Vec::new(),
        streams: vec![PrefixRecoveryStream {
            plan: PrefixRecoveryPlan {
                stream: "runtime".into(),
                source_epoch: 7,
                receiver_watermark: 0,
                head_segment_id: hex::encode(Sha256::digest(head.wire_bytes().unwrap())),
            },
            original_head: head.wire_bytes().unwrap(),
            retained_records: records,
            retained_record_signatures: BTreeMap::new(),
        }],
        signature: String::new(),
    };
    // Sign the same domain-separated canonical digest as the real source exporter.
    let mut bytes = b"xp-source-prefix-recovery-v1\0".to_vec();
    bytes.extend(serde_json::to_vec(&bundle).unwrap());
    bundle.signature = URL_SAFE_NO_PAD.encode(key.sign(&Sha256::digest(bytes)).to_bytes());
    let digest = bundle.digest().unwrap();
    for stream in &mut bundle.streams {
        for record in &stream.retained_records {
            stream.retained_record_signatures.insert(
                record_key(record).unwrap(),
                attest_record(&digest, record, &key).unwrap(),
            );
        }
    }
    bundle
}

fn aggregate() -> StoredRecord {
    let mut value = StoredRecord::from_record(
        11,
        11,
        &ReplicaCursor::new("node-a", 7, "runtime", 1).unwrap(),
        &record(b"retained-aggregate", false),
    );
    value.payload = serde_json::to_vec(&serde_json::json!({
        "algorithm":"sha256", "resolution":"five_minutes",
        "bucket_start_unix_seconds":0, "bucket_end_unix_seconds":299,
        "record_count":2, "first_sequence":0, "last_sequence":1,
        "payload_sha256":hex::encode([1_u8;32]), "complete":true,
    }))
    .unwrap();
    value
}

#[test]
fn explicit_prefix_recovery_archives_without_double_counting_and_acknowledges_original_head() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"existing", false)], None);
    let head = segment(&key, 2, vec![record(b"head", false)], Some([9; 32]));
    let mut runtime = load(directory.path());
    runtime
        .receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &first.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    assert!(matches!(
        runtime.receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &head.wire_bytes().unwrap(),
            12
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::SequenceGap { .. }
        ))
    ));
    let bundle = bundle(&head, vec![aggregate()]);
    let before = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    let fingerprint = runtime.prefix_recovery_fingerprint(&bundle).unwrap();
    assert_eq!(
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap(),
        before
    );
    let receipts = runtime
        .apply_prefix_recovery(&bundle, &fingerprint, 13, &["local".into()], "local")
        .unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].acknowledgement.sequence, 2);
    assert_eq!(
        runtime.prefix_recovery_evidence()[0].retained_records.len(),
        1
    );
    assert_eq!(
        runtime.prefix_recovery_evidence()[0].gaps[0].first_sequence,
        1
    );
    assert_eq!(
        runtime.prefix_recovery_evidence()[0].gaps[0]
            .reason
            .as_deref(),
        Some(LOSS_REASON)
    );
    // The overlapping source aggregate is not another canonical query/compaction contribution.
    assert_eq!(
        runtime
            .records_for_query(None, None, 0, 100, 0, 128)
            .unwrap()
            .len(),
        2
    );
    drop(runtime);
    let mut runtime = load(directory.path());
    assert_eq!(
        runtime.prefix_recovery_evidence()[0].retained_records.len(),
        1
    );
    // Ordinary source replay, rather than a fabricated local outbox edit, gets the real receipt.
    let receipt = runtime
        .receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &head.wire_bytes().unwrap(),
            14,
        )
        .unwrap();
    assert_eq!(receipt.acknowledgement.sequence, 2);
    let next = segment(
        &key,
        3,
        vec![record(b"next", false)],
        Some(head.segment_hash().unwrap()),
    );
    runtime
        .receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &next.wire_bytes().unwrap(),
            15,
        )
        .unwrap();
    assert!(
        runtime
            .apply_prefix_recovery(&bundle, &fingerprint, 16, &["local".into()], "local")
            .is_err()
    );
    assert_eq!(runtime.prefix_recovery_evidence().len(), 1);
    assert!(runtime.snapshot.history_truncated);
}

#[test]
fn changed_signed_inventory_and_watermark_fail_before_any_loss_or_archive_write() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"existing", false)], None);
    let head = segment(&key, 2, vec![record(b"head", false)], Some([9; 32]));
    let mut runtime = load(directory.path());
    runtime
        .receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &first.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    let mut bundle = bundle(&head, vec![aggregate()]);
    let fingerprint = runtime.prefix_recovery_fingerprint(&bundle).unwrap();
    let before = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    bundle.streams[0].retained_records[0].payload.push(0);
    assert!(
        runtime
            .apply_prefix_recovery(&bundle, &fingerprint, 12, &["local".into()], "local")
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap(),
        before
    );
    assert!(runtime.prefix_recovery_evidence().is_empty());
}

#[test]
fn archived_aggregate_obeys_the_existing_deletion_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"existing", false)], None);
    let head = segment(&key, 2, vec![record(b"head", false)], Some([9; 32]));
    let mut runtime = load(directory.path());
    runtime
        .receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &first.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    let bundle = bundle(&head, vec![aggregate()]);
    let fingerprint = runtime.prefix_recovery_fingerprint(&bundle).unwrap();
    runtime
        .apply_prefix_recovery(&bundle, &fingerprint, 12, &["local".into()], "local")
        .unwrap();
    let deletion = segment(
        &key,
        3,
        vec![record(b"retained-aggregate", true)],
        Some(head.segment_hash().unwrap()),
    );
    runtime
        .receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &deletion.wire_bytes().unwrap(),
            13,
        )
        .unwrap();
    assert!(
        runtime.prefix_recovery_evidence()[0]
            .retained_records
            .is_empty()
    );
    drop(runtime);
    assert!(
        load(directory.path()).prefix_recovery_evidence()[0]
            .retained_records
            .is_empty()
    );
}

#[test]
fn prefix_recovery_storage_failure_rolls_back_archive_gap_watermark_and_original_wire() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"existing", false)], None);
    let head = segment(&key, 2, vec![record(b"head", false)], Some([9; 32]));
    let mut runtime = load(directory.path());
    runtime
        .receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &first.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    let bundle = bundle(&head, vec![aggregate()]);
    let fingerprint = runtime.prefix_recovery_fingerprint(&bundle).unwrap();
    let before = serde_json::to_value(runtime.snapshot_for_persistence()).unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("history.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_prefix BEFORE UPDATE ON history_snapshots
         WHEN NEW.key='repository_replica'
         BEGIN SELECT RAISE(ABORT,'injected prefix commit failure'); END;",
        )
        .unwrap();
    assert!(
        runtime
            .apply_prefix_recovery(&bundle, &fingerprint, 12, &["local".into()], "local")
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(runtime.snapshot_for_persistence()).unwrap(),
        before
    );
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM repository_history_segments WHERE first_sequence=2",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(runtime);
    assert!(load(directory.path()).prefix_recovery_evidence().is_empty());
}

#[test]
fn source_prefix_export_binds_exact_pending_head_inventory_and_deletion_baseline() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let identity = identity(&key);
    let head_record = SyncRecord::new(
        "subject-a",
        "subject-a",
        "runtime.v1",
        1,
        b"node-history:node:subject-a:head".to_vec(),
        b"sample".to_vec(),
        false,
    );
    let head = segment(&key, 2, vec![head_record], Some([9; 32]));
    let runtime = load(directory.path());
    let wire = head.wire_bytes().unwrap();
    let id = hex::encode(Sha256::digest(&wire));
    runtime
        .storage
        .append_source_delivery_journal(&[
            crate::state::history_storage::SourceDeliveryJournalRow {
                id: id.clone(),
                stream: "runtime".into(),
                closed_at_unix_seconds: 11,
                identity: identity.clone(),
                wire,
            },
        ])
        .unwrap();
    let mut retained = aggregate();
    retained.subject_node_id = "subject-a".into();
    retained.observer_node_id = "subject-a".into();
    retained.record_key = b"node-history:node:subject-a:aggregate".to_vec();
    runtime
        .storage
        .upsert_repository_history_records(&[retained.sqlite_row().unwrap()])
        .unwrap();
    let plans = vec![PrefixRecoveryPlan {
        stream: "runtime".into(),
        source_epoch: 7,
        receiver_watermark: 0,
        head_segment_id: id,
    }];
    let exported = runtime
        .export_prefix_recovery(
            xp_test_fixtures::primary_cluster_id(),
            "local",
            identity.clone(),
            &key,
            "bound".into(),
            plans.clone(),
            0,
            &[],
        )
        .unwrap();
    exported.verify().unwrap();
    assert_eq!(exported.streams[0].retained_records.len(), 1);
    assert_eq!(
        exported.streams[0].retained_records[0].payload,
        aggregate().payload
    );
    let history = crate::node_history::NodeHistoryHandle::new(directory.path().join("source.json"));
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        history.clear_node("subject-a").await.unwrap();
        let (version, markers) = history.repository_deletion_snapshot("local").await.unwrap();
        let deleted = runtime
            .export_prefix_recovery(
                xp_test_fixtures::primary_cluster_id(),
                "local",
                identity,
                &key,
                "bound".into(),
                plans,
                version,
                &markers,
            )
            .unwrap();
        deleted.verify().unwrap();
        assert_ne!(deleted.digest().unwrap(), exported.digest().unwrap());
        let collector_directory = tempfile::tempdir().unwrap();
        let mut collector = load(collector_directory.path());
        let first = segment(&key, 0, vec![record(b"existing", false)], None);
        collector
            .receive_wire(
                xp_test_fixtures::primary_cluster_id(),
                &deleted.identity,
                &first.wire_bytes().unwrap(),
                11,
            )
            .unwrap();
        let fingerprint = collector.prefix_recovery_fingerprint(&deleted).unwrap();
        let receipts = collector
            .apply_prefix_recovery(&deleted, &fingerprint, 12, &["local".into()], "local")
            .unwrap();
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[1].acknowledgement.sequence, 2);
        assert!(
            collector.prefix_recovery_evidence()[0]
                .retained_records
                .is_empty()
        );
        assert_eq!(
            collector
                .records_for_query(None, None, 0, 100, 0, 128)
                .unwrap()
                .len(),
            1
        );
        // The baseline and original head share one transaction and preserve source deletion.
        let baseline = SignedSegment::from_wire(&deleted.deletion_baseline_wire).unwrap();
        assert!(
            baseline
                .canonical()
                .records()
                .iter()
                .all(SyncRecord::is_tombstone)
        );
    });
}

#[test]
fn persisted_prefix_provenance_rejects_tampering_and_survives_selective_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"existing", false)], None);
    let head = segment(&key, 2, vec![record(b"head", false)], Some([9; 32]));
    let mut kept = aggregate();
    kept.record_key = b"kept-aggregate".to_vec();
    let bundle = bundle(&head, vec![aggregate(), kept.clone()]);
    let mut runtime = load(directory.path());
    runtime
        .receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &first.wire_bytes().unwrap(),
            11,
        )
        .unwrap();
    let fingerprint = runtime.prefix_recovery_fingerprint(&bundle).unwrap();
    runtime
        .apply_prefix_recovery(&bundle, &fingerprint, 12, &["local".into()], "local")
        .unwrap();
    drop(runtime);
    let mut runtime = load(directory.path());
    let evidence = runtime.prefix_recovery_evidence()[0].clone();
    evidence.verify().unwrap();
    let mut changed = evidence.clone();
    changed.retained_records[0].payload.push(0);
    assert!(changed.verify().is_err());
    let mut changed = evidence.clone();
    changed.bundle_digest = hex::encode([9; 32]);
    assert!(changed.verify().is_err());
    let mut changed = evidence;
    changed.retained_record_signatures.clear();
    assert!(changed.verify().is_err());
    let deletion = segment(
        &key,
        3,
        vec![record(b"retained-aggregate", true)],
        Some(head.segment_hash().unwrap()),
    );
    runtime
        .receive_wire(
            xp_test_fixtures::primary_cluster_id(),
            &identity,
            &deletion.wire_bytes().unwrap(),
            13,
        )
        .unwrap();
    drop(runtime);
    let runtime = load(directory.path());
    let evidence = &runtime.prefix_recovery_evidence()[0];
    evidence.verify().unwrap();
    assert_eq!(evidence.retained_records.len(), 1);
    assert_eq!(evidence.retained_record_signatures.len(), 1);
    assert_eq!(evidence.retained_records[0].payload, kept.payload);
    assert_eq!(evidence.retained_records[0].record_key, kept.record_key);
}
