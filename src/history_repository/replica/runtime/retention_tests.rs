use super::{
    tests::{identity, identity_for, load, record, segment_at, signing_key},
    *,
};
use crate::{
    history_sync::{CanonicalSegment, SyncRecord},
    state::history_repository::{
        control::{
            DEFAULT_HISTORY_REPOSITORY_QUOTA_BYTES, HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES,
            HistoryWriteAvailability,
        },
        query::Completeness,
    },
};
use ed25519_dalek::SigningKey;

#[test]
fn sqlite_retention_progresses_for_mixed_buckets_sharing_a_dense_timestamp() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let observed_at = 10_000_u64;
    let now = observed_at
        .saturating_add(policy.minute_retention_seconds())
        .saturating_add(1);
    let count = RETENTION_COMPACTION_PAGE_SIZE + RETENTION_COMPACTION_BUCKET_LOOKAHEAD + 32;
    let mut runtime = load(temporary.path());
    let rows = (0..u64::try_from(count).expect("count"))
        .map(|sequence| {
            let traffic = sequence % 2 == 0;
            StoredRecord {
                observed_at_unix_seconds: observed_at,
                received_at_unix_seconds: now,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: if traffic { "traffic" } else { "runtime" }.to_owned(),
                sequence,
                subject_node_id: if traffic { "subject-a" } else { "subject-b" }.to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: if traffic { "traffic.v1" } else { "runtime.v1" }.to_owned(),
                schema_version: 1,
                record_key: format!("mixed-{sequence}").into_bytes(),
                payload: b"sample".to_vec(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed mixed dense buckets");

    for _ in 0..4 {
        runtime
            .prepare_for_replication(now)
            .expect("bounded mixed compaction page");
        if runtime.snapshot.retention_compaction_cursor.is_none() {
            break;
        }
    }
    assert!(runtime.snapshot.retention_compaction_cursor.is_none());
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("record count"),
        2,
        "each mixed retention bucket remains one aggregate after bounded continuation"
    );
}

#[test]
fn sqlite_retention_preserves_interleaved_subject_buckets_across_pages() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let observed_at = 10_000_u64;
    let now = observed_at
        .saturating_add(policy.minute_retention_seconds())
        .saturating_add(1);
    let count = RETENTION_COMPACTION_PAGE_SIZE + RETENTION_COMPACTION_BUCKET_LOOKAHEAD + 32;
    let mut runtime = load(temporary.path());
    let rows = (0..u64::try_from(count).expect("count"))
        .map(|sequence| {
            let subject = if sequence % 2 == 0 {
                "subject-a"
            } else {
                "subject-b"
            };
            StoredRecord {
                observed_at_unix_seconds: observed_at,
                received_at_unix_seconds: now,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: subject.to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("interleaved-{sequence}").into_bytes(),
                payload: b"sample".to_vec(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed interleaved buckets");

    runtime
        .prepare_for_replication(now)
        .expect("start interleaved compaction");
    while runtime.snapshot.retention_compaction_cursor.is_some()
        || runtime.snapshot.retention_compaction_continuation.is_some()
    {
        runtime
            .prepare_for_replication(now)
            .expect("compact interleaved page");
    }
    let records = runtime
        .sqlite_records(None, None, None, 0, 8)
        .expect("read compacted records");
    let mut counts = std::collections::BTreeMap::new();
    for record in records {
        let payload: serde_json::Value =
            serde_json::from_slice(&record.payload).expect("aggregate payload");
        *counts.entry(record.subject_node_id).or_insert(0_u64) +=
            payload["record_count"].as_u64().expect("aggregate count");
    }
    assert_eq!(
        counts.get("subject-a"),
        Some(&(u64::try_from(count).unwrap() / 2))
    );
    assert_eq!(
        counts.get("subject-b"),
        Some(&(u64::try_from(count).unwrap() / 2))
    );
}

#[test]
fn late_history_record_discards_an_unfinished_sqlite_compaction_continuation() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let observed_at = 10_000_u64;
    let now = observed_at
        .saturating_add(policy.minute_retention_seconds())
        .saturating_add(1);
    let count = RETENTION_COMPACTION_PAGE_SIZE + RETENTION_COMPACTION_BUCKET_LOOKAHEAD + 32;
    let mut runtime = load(temporary.path());
    let rows = (0..u64::try_from(count).expect("count"))
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: observed_at,
                received_at_unix_seconds: now,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("late-{sequence}").into_bytes(),
                payload: b"sample".to_vec(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed dense bucket");
    runtime
        .prepare_for_replication(now)
        .expect("create continuation");
    assert!(runtime.snapshot.retention_compaction_continuation.is_some());

    let late_segment = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 0).expect("cursor"),
        vec![SyncRecord::new(
            "subject-a",
            "node-a",
            "traffic.v1",
            1,
            b"late-record".to_vec(),
            b"sample".to_vec(),
            false,
        )],
        None,
        observed_at,
        observed_at,
    )
    .expect("late segment");
    runtime.reopen_retention_compaction_for_late_record(&late_segment);
    assert!(runtime.snapshot.retention_compaction_cursor.is_none());
    assert!(runtime.snapshot.retention_compaction_continuation.is_none());
}

#[test]
fn continuous_ingestion_does_not_restart_sqlite_retention_compaction() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let first_observed = 1_000_u64;
    let count = RETENTION_COMPACTION_PAGE_SIZE * 3 + 1;
    let now = first_observed
        .saturating_add(u64::try_from(count).expect("count") * 60)
        .saturating_add(policy.minute_retention_seconds())
        .saturating_add(1);
    let mut runtime = load(temporary.path());
    let rows = (0..u64::try_from(count).expect("count"))
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: first_observed.saturating_add(sequence * 60),
                received_at_unix_seconds: now,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "runtime".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "runtime.v1".to_owned(),
                schema_version: 1,
                record_key: format!("continuous-{sequence}").into_bytes(),
                payload: b"sample".to_vec(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite history row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed multiple compaction pages");
    runtime
        .prepare_for_replication(now)
        .expect("compact first page");
    let first_cursor = runtime
        .snapshot
        .retention_compaction_cursor
        .clone()
        .expect("first cursor");

    let key = SigningKey::from_bytes(&[13; 32]);
    let identity = identity_for(&key, "node-b");
    let live = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-b", 8, "runtime", 0).expect("cursor"),
        vec![record(b"newer-live-sample", false)],
        None,
        now,
        now,
    )
    .expect("live segment")
    .sign(&key)
    .expect("sign live segment");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &live.wire_bytes().expect("wire"),
            now,
        )
        .expect("receive newer live segment");
    let advanced_cursor = runtime
        .snapshot
        .retention_compaction_cursor
        .clone()
        .expect("cursor remains while old pages remain");
    assert!(advanced_cursor.observed_start_unix_seconds > first_cursor.observed_start_unix_seconds);
}

#[test]
fn sqlite_signed_segment_cache_expires_after_the_minute_detail_window() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let now = policy.minute_retention_seconds().saturating_add(10_000);
    let observed_at = now
        .saturating_sub(policy.minute_retention_seconds())
        .saturating_sub(1);
    let key = signing_key();
    let identity = identity(&key);
    let signed = segment_at(
        &key,
        0,
        vec![record(b"expired-anti-entropy-cache", false)],
        None,
        observed_at,
        observed_at,
    );
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &signed.wire_bytes().expect("wire"),
            now,
        )
        .expect("receive old segment");
    assert_eq!(
        runtime
            .storage
            .repository_history_segment_count()
            .expect("segment count"),
        0,
        "canonical rows remain tiered in SQLite while raw signed wire is short-lived"
    );
}

#[test]
fn peer_initial_backfill_checkpoint_survives_restart() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    let mut streams = std::collections::BTreeMap::new();
    streams.insert("traffic-backfill-node-b".to_owned(), (64, Some([7; 32])));
    runtime
        .update_initial_peer_backfill_checkpoint(
            "node-b",
            Some("opaque-page-cursor".to_owned()),
            streams.clone(),
            true,
            false,
        )
        .expect("persist peer checkpoint");
    runtime
        .update_initial_peer_summary_checkpoint(
            "node-b",
            Some("segment-1".to_owned()),
            vec!["segment-2".to_owned()],
            Some("segment-3".to_owned()),
            false,
            true,
        )
        .expect("persist summary checkpoint");
    runtime
        .update_initial_peer_backfill_checkpoint(
            "node-b",
            Some("tiered-page-2".to_owned()),
            streams.clone(),
            true,
            false,
        )
        .expect("preserve summary checkpoint");
    drop(runtime);

    let restored = load(temporary.path());
    assert_eq!(
        restored.initial_peer_backfill_checkpoint("node-b"),
        Some(InitialPeerBackfillCheckpoint {
            page_cursor: Some("tiered-page-2".to_owned()),
            stream_state: streams,
            saw_history: true,
            completed: false,
            epoch: 0,
            summary_cursor: Some("segment-1".to_owned()),
            summary_pending_segment_ids: vec!["segment-2".to_owned()],
            summary_pending_next_cursor: Some("segment-3".to_owned()),
            summary_complete: false,
            summary_requires_tiered_backfill: true,
            ..InitialPeerBackfillCheckpoint::default()
        })
    );
}

#[test]
fn peer_initial_backfill_restart_reuses_the_epoch_but_resets_the_page_state() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime
        .update_initial_peer_backfill_checkpoint(
            "node-b",
            Some("stale-export-cursor".to_owned()),
            std::collections::BTreeMap::from([("traffic-backfill-node-b".to_owned(), (9, None))]),
            true,
            false,
        )
        .expect("checkpoint state");
    let epoch = runtime
        .initial_peer_backfill_epoch("cluster-a", "node-a", "node-b")
        .expect("allocate import epoch");

    runtime
        .restart_initial_peer_backfill("node-b")
        .expect("restart stale export");

    assert_eq!(
        runtime
            .initial_peer_backfill_checkpoint("node-b")
            .expect("reset checkpoint"),
        InitialPeerBackfillCheckpoint {
            epoch,
            ..InitialPeerBackfillCheckpoint::default()
        }
    );
}

#[test]
fn tiered_backfill_respects_low_space_and_quota_write_guards() {
    let record = || RepositoryTieredBackfillRecord {
        observed_at_unix_seconds: 100,
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "traffic".to_owned(),
        sequence: 0,
        subject_node_id: "node-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "traffic.v1".to_owned(),
        schema_version: 1,
        record_key: b"tiered-capacity".to_vec(),
        payload: b"sample".to_vec(),
        tombstone: false,
    };
    for (used_bytes, available_bytes, expected) in [
        (
            0,
            HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES - 1,
            HistoryWriteAvailability::DegradedLowSpace,
        ),
        (
            DEFAULT_HISTORY_REPOSITORY_QUOTA_BYTES,
            HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES,
            HistoryWriteAvailability::QuotaReached,
        ),
    ] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut runtime = load(temporary.path());
        runtime
            .force_capacity_for_test(used_bytes, available_bytes)
            .expect("capacity");
        assert!(matches!(
            runtime.import_tiered_backfill_records(
                vec![record()],
                101,
                &["repo-a".to_owned()],
                "repo-a",
            ),
            Err(RepositoryRuntimeError::WriteStopped(availability)) if availability == expected
        ));
        assert_eq!(
            runtime
                .storage
                .repository_history_record_count()
                .expect("record count"),
            0,
            "rejected tiered import does not persist history"
        );
    }
}

#[test]
fn sqlite_query_marks_an_incomplete_aggregate_beyond_the_first_compaction_page_partial() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let count = RETENTION_COMPACTION_PAGE_SIZE + 1;
    let rows = (0..u64::try_from(count).expect("count"))
        .map(|sequence| {
            let observed_at = 1_000_u64.saturating_add(sequence * 60 * 60);
            StoredRecord {
                observed_at_unix_seconds: observed_at,
                received_at_unix_seconds: 2_000_000,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("aggregate-{sequence}").into_bytes(),
                payload: serde_json::to_vec(&serde_json::json!({
                    "algorithm": "sha256",
                    "resolution": "hour",
                    "bucket_start_unix_seconds": observed_at,
                    "bucket_end_unix_seconds": observed_at + 59 * 60,
                    "record_count": 1,
                    "first_sequence": sequence,
                    "last_sequence": sequence,
                    "payload_sha256": "00",
                    "complete": sequence + 1 != u64::try_from(count).expect("count"),
                }))
                .expect("aggregate payload"),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite history row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed aggregate pages");
    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(
                1_000,
                1_000_u64.saturating_add(u64::try_from(count).expect("count") * 60 * 60),
                10,
            )
            .expect("query"),
            LocalQueryMetadata::current_window(2_000_000),
        )
        .expect("query SQLite aggregates");
    assert_eq!(response.plan().completeness(), Completeness::Partial);
}

#[test]
fn tiered_sqlite_history_exports_with_a_bounded_keyset_cursor() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let rows = (0..2_u64)
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: 1_000 + sequence,
                received_at_unix_seconds: 2_000,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("tiered-{sequence}").into_bytes(),
                payload: format!("tiered payload {sequence}").into_bytes(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed tiered rows");
    let first = runtime
        .tiered_backfill_page(None, 1, u64::MAX, 100)
        .expect("first bounded tiered page");
    assert_eq!(first.records.len(), 1);
    let second = runtime
        .tiered_backfill_page(first.next_cursor.as_deref(), 1, u64::MAX, 100)
        .expect("second bounded tiered page");
    assert_eq!(second.records.len(), 1);
    assert_ne!(first.records[0].record_key, second.records[0].record_key);
}

#[test]
fn tiered_sqlite_history_exports_respect_the_byte_budget() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let rows = (0..2_u64)
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: 1_000 + sequence,
                received_at_unix_seconds: 2_000,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("large-tiered-{sequence}").into_bytes(),
                payload: vec![b'x'; 150 * 1024],
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed large tiered rows");

    let first = runtime
        .tiered_backfill_page(None, 128, u64::MAX, 100)
        .expect("bounded tiered page");
    assert_eq!(first.records.len(), 1);
    assert!(first.next_cursor.is_some());
}
