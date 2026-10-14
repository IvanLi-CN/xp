use super::*;
use crate::state::history_repository::replica::RepositoryReplicaSegment;
use crate::state::history_storage::SourceDeliveryAttempt;
use sha2::Sha256;

#[test]
fn delivery_metrics_count_generated_and_durable_ack_bytes_across_restart() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());

    for sequence in 0..3_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    vec![u8::try_from(sequence + 1).unwrap(); 1024],
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }

    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let rows = storage.source_delivery_journal().expect("read journal");
    let generated_bytes = rows.iter().map(|row| row.wire.len() as u64).sum::<u64>();
    let metrics = storage
        .repository_delivery_metrics()
        .expect("read delivery metrics");
    assert_eq!(metrics.generated_segments, 3);
    assert_eq!(metrics.generated_bytes, generated_bytes);
    assert_eq!(metrics.acknowledged_segments, 0);

    runtime
        .record_local_source_delivery_attempt(SourceDeliveryAttempt {
            now_unix_seconds: 199,
            attempts: 3,
            successes: 2,
            outcome: "transport_error",
            target_repository_id: None,
        })
        .expect("record source delivery attempt");
    let metrics = storage
        .repository_delivery_metrics()
        .expect("read source attempt metrics");
    assert_eq!(metrics.source_delivery_attempts, 3);
    assert_eq!(metrics.source_delivery_successes, 2);
    assert_eq!(metrics.source_transport_errors, 1);
    assert_eq!(metrics.last_attempt_at, Some(199));

    runtime
        .acknowledge_local_source_segment_via(&rows[0].wire, 200, "direct")
        .expect("acknowledge source segment");
    let metrics = storage
        .repository_delivery_metrics()
        .expect("read acknowledged metrics");
    assert_eq!(metrics.acknowledged_segments, 1);
    assert_eq!(metrics.acknowledged_bytes, rows[0].wire.len() as u64);

    drop(runtime);
    let _restarted = load(temporary.path());
    let metrics = crate::state::history_repository::HistoryStorage::open(temporary.path())
        .repository_delivery_metrics()
        .expect("read restarted delivery metrics");
    assert_eq!(metrics.generated_segments, 3);
    assert_eq!(metrics.generated_bytes, generated_bytes);
    assert_eq!(metrics.acknowledged_segments, 1);
    assert_eq!(metrics.acknowledged_bytes, rows[0].wire.len() as u64);
    assert_eq!(
        crate::state::history_repository::HistoryStorage::open(temporary.path())
            .source_delivery_journal()
            .expect("read restarted journal")
            .len(),
        2
    );
}

#[test]
fn delivery_window_requires_both_segments_and_bytes_for_each_stage() {
    let start = crate::state::history_storage::RepositoryDeliveryMetrics::default();
    let mut end = start.clone();
    end.generated_segments = 10;
    end.generated_bytes = 1_000;
    end.acknowledged_segments = 100;
    end.acknowledged_bytes = 10_000;
    end.applied_segments = 100;
    end.applied_bytes = 10_000;
    let window = end
        .window_since(
            &start,
            crate::state::history_storage::DELIVERY_RATE_WINDOW_SECONDS,
        )
        .expect("valid five-minute delivery window");
    assert!(window.source_ack_meets_ten_x());
    assert!(window.repository_apply_meets_ten_x());

    assert!(
        !end.window_since(&start, 299)
            .expect("valid shorter delivery window")
            .source_ack_meets_ten_x()
    );

    end.applied_bytes = 9_999;
    assert!(
        !end.window_since(
            &start,
            crate::state::history_storage::DELIVERY_RATE_WINDOW_SECONDS,
        )
        .expect("valid five-minute delivery window")
        .repository_apply_meets_ten_x()
    );
}

#[test]
fn runtime_status_exposes_completed_five_minute_window() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[17; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
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
                b"window".to_vec(),
                b"payload".to_vec(),
                false,
            )],
            0,
        )
        .expect("queue source segment");
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let started_at = storage
        .repository_delivery_metrics()
        .expect("read metrics")
        .last_progress_at
        .expect("generated progress timestamp");
    runtime
        .record_local_source_delivery_attempt(SourceDeliveryAttempt {
            now_unix_seconds: started_at
                + crate::state::history_storage::DELIVERY_RATE_WINDOW_SECONDS,
            attempts: 1,
            successes: 0,
            outcome: "transport_error",
            target_repository_id: Some("repository-b".to_owned()),
        })
        .expect("rotate delivery window");
    let metrics = storage
        .repository_delivery_metrics_at(Some(
            started_at + crate::state::history_storage::DELIVERY_RATE_WINDOW_SECONDS,
        ))
        .expect("read completed window");
    let window = metrics
        .last_completed_window
        .expect("status should expose completed window");
    assert_eq!(window.elapsed_seconds, 300);
    assert_eq!(window.generated_segments, 1);
    assert_eq!(window.generated_bytes, metrics.generated_bytes);
    assert_eq!(window.acknowledged_segments, 0);
    assert!(!window.meets_ten_x);
    assert!(
        metrics
            .delivery_scopes
            .iter()
            .any(|scope| scope.target_repository_id.is_empty() && scope.generated_segments == 1)
    );

    let rows = storage
        .source_delivery_journal()
        .expect("read source journal");
    runtime
        .acknowledge_local_source_segment_via(&rows[0].wire, started_at + 1, "repository-b")
        .expect("acknowledge scoped source segment");
    let scoped_metrics = storage
        .repository_delivery_metrics_at(Some(
            started_at + crate::state::history_storage::DELIVERY_RATE_WINDOW_SECONDS + 1,
        ))
        .expect("read scoped completed window");
    let scoped_window = scoped_metrics
        .delivery_scopes
        .iter()
        .find(|scope| scope.target_repository_id == "repository-b")
        .and_then(|scope| scope.last_completed_window)
        .expect("status should expose scoped completed window");
    assert_eq!(scoped_window.generated_segments, 0);
    assert_eq!(scoped_window.acknowledged_segments, 1);
    assert!(!scoped_window.meets_ten_x);
}

#[test]
fn source_delivery_journal_state_tracks_upserts_and_ack_path() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    for sequence in 0..3_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    vec![u8::try_from(sequence + 1).unwrap(); 1024],
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }

    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let rows = storage.source_delivery_journal().expect("read journal");
    let expected_bytes = rows.iter().map(|row| row.wire.len() as u64).sum::<u64>();
    storage
        .append_source_delivery_journal(&rows)
        .expect("upsert journal rows");
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize upserted journal");
    assert_eq!(summary.pending_segments, rows.len());
    assert_eq!(summary.pending_bytes, expected_bytes);

    let restored_directory = tempfile::tempdir().expect("restored directory");
    let restored_storage =
        crate::state::history_repository::HistoryStorage::open(restored_directory.path());
    restored_storage
        .restore_source_delivery_journal(&rows)
        .expect("restore legacy journal rows");
    assert_eq!(
        restored_storage
            .repository_delivery_metrics()
            .expect("read restored metrics")
            .generated_segments,
        0
    );

    runtime
        .acknowledge_local_source_segment_via(&rows[0].wire, 200, "direct")
        .expect("acknowledge journal row");
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize acknowledged page");
    assert_eq!(summary.pending_segments, rows.len() - 1);
    assert_eq!(
        summary.pending_bytes,
        expected_bytes - rows[0].wire.len() as u64
    );
    assert_eq!(summary.last_acknowledged_at, Some(200));
    assert_eq!(summary.last_delivery_path.as_deref(), Some("direct"));

    storage
        .acknowledge_source_delivery_journal(
            &[hex::encode(Sha256::digest(&rows[0].wire))],
            Some(300),
            Some("dynamic_relay"),
        )
        .expect("repeat acknowledgement is idempotent");
    let summary = storage
        .source_delivery_journal_summary()
        .expect("summarize repeated acknowledgement");
    assert_eq!(summary.last_acknowledged_at, Some(200));
    assert_eq!(summary.last_delivery_path.as_deref(), Some("direct"));
}

#[test]
fn source_delivery_batch_ack_removes_page_in_one_checkpoint() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut runtime = load(temporary.path());
    for sequence in 0..3_u64 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                source_identity.clone(),
                &signing_key,
                vec![SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    format!("runtime:{sequence}").into_bytes(),
                    b"sample".to_vec(),
                    false,
                )],
                sequence,
            )
            .expect("queue source segment");
    }

    let rows = crate::state::history_repository::HistoryStorage::open(temporary.path())
        .source_delivery_journal()
        .expect("read journal");
    let page = rows
        .iter()
        .map(|row| RepositoryReplicaSegment {
            identity: row.identity.clone(),
            wire: row.wire.clone(),
        })
        .collect::<Vec<_>>();
    runtime
        .acknowledge_local_source_segments_via(&page, 200, "direct")
        .expect("acknowledge source page");

    let summary = crate::state::history_repository::HistoryStorage::open(temporary.path())
        .source_delivery_journal_summary()
        .expect("summarize acknowledged page");
    assert_eq!(summary.pending_segments, 0);
    assert_eq!(summary.pending_bytes, 0);
    assert_eq!(summary.last_acknowledged_at, Some(200));
    assert_eq!(summary.last_delivery_path.as_deref(), Some("direct"));
    assert!(runtime.local_source_pending_segments().is_empty());
}

#[test]
fn local_batch_receiver_commits_apply_once_and_replays_idempotently() {
    let source_dir = tempfile::tempdir().expect("source directory");
    let collector_dir = tempfile::tempdir().expect("collector directory");
    let key = SigningKey::from_bytes(&[11; 32]);
    let source_identity = identity();
    let mut source = load(source_dir.path());
    let mut records = Vec::new();
    for sequence in 0..2_u64 {
        records.push(SyncRecord::new(
            "node-a",
            "node-a",
            "runtime.v1",
            1,
            format!("runtime:{sequence}").into_bytes(),
            format!("payload:{sequence}").into_bytes(),
            false,
        ));
    }
    let queued = source
        .queue_local_source_segments_for_repositories(
            "cluster-a",
            source_identity,
            &key,
            records,
            100,
            &["repo-a".to_owned()],
        )
        .expect("queue source page");
    assert_eq!(queued.len(), 1);

    let mut collector = load(collector_dir.path());
    let segments = queued
        .iter()
        .map(|segment| RepositoryReplicaSegment {
            identity: segment.identity.clone(),
            wire: segment.wire.clone(),
        })
        .collect::<Vec<_>>();
    let expected_wire_bytes = segments
        .iter()
        .map(|segment| segment.wire.len() as u64)
        .sum::<u64>();
    let receipts = collector
        .receive_wire_batch_from_repository_with_gaps(
            "cluster-a",
            &segments,
            &[],
            100,
            &["repo-a".to_owned()],
            "repo-a",
        )
        .expect("receive source page in one transaction");
    assert_eq!(receipts.len(), segments.len());
    let metrics = crate::state::history_repository::HistoryStorage::open(collector_dir.path())
        .repository_delivery_metrics()
        .expect("read apply metrics");
    assert_eq!(metrics.applied_segments, segments.len() as u64);
    assert_eq!(metrics.applied_bytes, expected_wire_bytes);

    collector
        .receive_wire_batch_from_repository_with_gaps(
            "cluster-a",
            &segments,
            &[],
            101,
            &["repo-a".to_owned()],
            "repo-a",
        )
        .expect("replay source page idempotently");
    let metrics = crate::state::history_repository::HistoryStorage::open(collector_dir.path())
        .repository_delivery_metrics()
        .expect("read replay metrics");
    assert_eq!(metrics.applied_segments, segments.len() as u64);
    assert_eq!(metrics.applied_bytes, expected_wire_bytes);
}
