use ed25519_dalek::SigningKey;

use super::{
    tests::{
        identity, identity_for, load, record, segment, segment_at, signing_key, traffic_record,
    },
    *,
};
use crate::{
    history_sync::{CanonicalSegment, SyncRecord},
    state::history_repository::{
        control::{HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES, HistoryWriteAvailability},
        query::Completeness,
    },
};

#[test]
fn tiered_sqlite_backfill_excludes_the_signed_repair_window() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let repair_cache_cutoff = 10_000_u64;
    let rows = [repair_cache_cutoff - 1, repair_cache_cutoff]
        .into_iter()
        .enumerate()
        .map(|(sequence, observed_at)| {
            StoredRecord {
                observed_at_unix_seconds: observed_at,
                received_at_unix_seconds: repair_cache_cutoff,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence: u64::try_from(sequence).expect("sequence"),
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

    let page = runtime
        .tiered_backfill_page(None, 8, repair_cache_cutoff, repair_cache_cutoff)
        .expect("export older tier");
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].record_key, b"tiered-0");
}

#[test]
fn tiered_sqlite_backfill_uses_a_stable_received_watermark_across_pages() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let repair_cache_cutoff = 10_000_u64;
    let initial_rows = [1_000_u64, 2_000]
        .into_iter()
        .enumerate()
        .map(|(sequence, observed_at)| {
            StoredRecord {
                observed_at_unix_seconds: observed_at,
                received_at_unix_seconds: 100,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence: u64::try_from(sequence).expect("sequence"),
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("initial-{sequence}").into_bytes(),
                payload: b"initial".to_vec(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&initial_rows)
        .expect("seed initial export snapshot");
    let first = runtime
        .tiered_backfill_page(None, 1, repair_cache_cutoff, repair_cache_cutoff)
        .expect("first export page");

    let late_row = StoredRecord {
        observed_at_unix_seconds: 500,
        received_at_unix_seconds: 101,
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "traffic".to_owned(),
        sequence: 2,
        subject_node_id: "subject-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "traffic.v1".to_owned(),
        schema_version: 1,
        record_key: b"inserted-after-page-one".to_vec(),
        payload: b"late".to_vec(),
        tombstone: false,
    }
    .sqlite_row()
    .expect("SQLite row");
    runtime
        .storage
        .upsert_repository_history_records(&[late_row])
        .expect("insert late row");

    let second = runtime
        .tiered_backfill_page(
            first.next_cursor.as_deref(),
            8,
            repair_cache_cutoff,
            repair_cache_cutoff,
        )
        .expect("second export page");
    assert_eq!(second.records.len(), 1);
    assert_eq!(second.records[0].record_key, b"initial-1");
    let fresh = runtime
        .tiered_backfill_page(None, 8, repair_cache_cutoff, repair_cache_cutoff)
        .expect("fresh export snapshot");
    assert!(
        fresh
            .records
            .iter()
            .any(|record| record.record_key == b"inserted-after-page-one")
    );
}

#[test]
fn active_tiered_export_defers_representation_rewrites_until_the_final_page() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let now = 20_000_u64
        .saturating_add(policy.minute_retention_seconds())
        .saturating_add(1);
    let repair_cache_cutoff = now.saturating_sub(policy.minute_retention_seconds());
    let mut runtime = load(temporary.path());
    let rows = (0..2_u64)
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: 20_000,
                received_at_unix_seconds: 100,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("rewrite-{sequence}").into_bytes(),
                payload: b"raw".to_vec(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed export rows");
    let first = runtime
        .tiered_backfill_page(None, 1, repair_cache_cutoff, now)
        .expect("first export page");
    assert!(first.next_cursor.is_some());

    runtime
        .prepare_for_replication(now)
        .expect("defer compaction while export is active");
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("record count"),
        2
    );

    runtime
        .tiered_backfill_page(first.next_cursor.as_deref(), 1, repair_cache_cutoff, now)
        .expect("final export page");
    runtime
        .prepare_for_replication(now)
        .expect("compact after export finishes");
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("record count"),
        1
    );
}

#[test]
fn expired_tiered_export_cursor_must_restart_at_page_zero() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let rows = (0..2_u64)
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: 1_000 + sequence,
                received_at_unix_seconds: 100,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("lease-{sequence}").into_bytes(),
                payload: b"lease".to_vec(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed lease rows");
    let first = runtime
        .tiered_backfill_page(None, 1, u64::MAX, 100)
        .expect("first export page");
    let cursor = first.next_cursor.expect("continuation cursor");
    assert!(matches!(
        runtime.tiered_backfill_page(Some(&cursor), 1, u64::MAX, 100 + 15 * 60 + 1,),
        Err(RepositoryRuntimeError::StateLimitExceeded)
    ));
    let restarted = runtime
        .tiered_backfill_page(None, 1, u64::MAX, 100 + 15 * 60 + 1)
        .expect("restart export after lease expiry");
    assert_eq!(restarted.records[0].record_key, b"lease-0");
}

#[test]
fn local_history_backfill_acknowledgement_and_checkpoint_restart_together() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let segment = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity,
            &key,
            vec![record(b"historical-record", false)],
            1_000,
        )
        .expect("queue source segment")
        .expect("segment");

    runtime
        .acknowledge_local_source_segment_and_checkpoint_backfill(
            &segment.wire,
            Some("history-page-1".to_owned()),
            false,
        )
        .expect("persist acknowledgement and checkpoint");
    let restored = load(temporary.path());
    assert!(restored.local_source_pending_segments().is_empty());
    assert_eq!(
        restored.local_history_backfill_cursor(),
        Some("history-page-1"),
        "a restart cannot reassign a source sequence to the acknowledged page"
    );
}

#[test]
fn local_history_backfill_rejects_a_page_before_partial_stream_enqueue() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    let unrelated_live = runtime
        .queue_local_source_segment(
            "cluster-a",
            identity.clone(),
            &key,
            vec![traffic_record(b"unrelated-live")],
            1_000,
        )
        .expect("queue unrelated live segment")
        .expect("unrelated live segment");
    let backfill = runtime
        .queue_local_history_backfill_segments(
            "cluster-a",
            identity.clone(),
            &key,
            vec![record(b"historical-traffic", false)],
            1_001,
        )
        .expect("queue historical page");
    assert_eq!(backfill.len(), 1);
    assert_ne!(backfill[0].wire, unrelated_live.wire);
    runtime
        .acknowledge_local_source_segment(&backfill[0].wire)
        .expect("drain historical segment");

    for sequence in 0..8 {
        runtime
            .queue_local_source_segment(
                "cluster-a",
                identity.clone(),
                &key,
                vec![record(format!("runtime-{sequence}").as_bytes(), false)],
                1_002 + sequence,
            )
            .expect("fill traffic outbox");
    }
    assert!(matches!(
        runtime.queue_local_history_backfill_segments(
            "cluster-a",
            identity,
            &key,
            vec![record(b"historical-after-backpressure", false)],
            1_100,
        ),
        Err(RepositoryRuntimeError::StateLimitExceeded)
    ));
    assert_eq!(runtime.local_source_pending_segments().len(), 2);
}

#[test]
fn failed_replica_commit_does_not_leave_rows_or_checkpoint_behind() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let storage = crate::state::history_repository::HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).expect("runtime");
    storage
        .set_query_only_for_test(true)
        .expect("enable SQLite write failure");

    let error = runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment(&key, 0, vec![record(b"must-not-survive", false)], None)
                .wire_bytes()
                .expect("wire"),
            11,
        )
        .expect_err("query-only SQLite must reject the atomic commit");
    assert!(matches!(error, RepositoryRuntimeError::Storage(_)));
    assert_eq!(storage.repository_history_record_count().expect("count"), 0);
    let status = runtime
        .runtime_status(12)
        .expect("degraded status remains readable");
    assert_eq!(status.storage_mode, "sqlite_degraded");

    storage
        .set_query_only_for_test(false)
        .expect("disable SQLite write failure");
    let restored = RepositoryReplicaRuntime::load(storage).expect("reload runtime");
    assert!(restored.snapshot.receiver.is_none());
    assert_eq!(
        restored
            .storage
            .repository_history_record_count()
            .expect("reloaded count"),
        0
    );
}

#[test]
fn legacy_sqlite_rows_without_aggregate_metadata_are_partial() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let mut legacy_row = StoredRecord {
        observed_at_unix_seconds: 1_000,
        received_at_unix_seconds: 1_001,
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "traffic".to_owned(),
        sequence: 0,
        subject_node_id: "subject-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "traffic.v1".to_owned(),
        schema_version: 1,
        record_key: b"legacy-row".to_vec(),
        payload: b"legacy payload".to_vec(),
        tombstone: false,
    }
    .sqlite_row()
    .expect("SQLite row");
    legacy_row.aggregate_complete = None;
    legacy_row.aggregate_start_unix_seconds = None;
    legacy_row.aggregate_end_unix_seconds = None;
    runtime
        .storage
        .upsert_repository_history_records(&[legacy_row])
        .expect("seed legacy SQLite row");

    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(999, 1_001, 10).expect("query"),
            LocalQueryMetadata::current_window(1_001),
        )
        .expect("query legacy SQLite row");
    assert_eq!(response.plan().completeness(), Completeness::Partial);
}

#[test]
fn repository_retention_marks_aggregates_incomplete_for_permanent_gaps() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let observed_at = 1_000_u64;
    let now = observed_at + 7 * 24 * 60 * 60 + 2;
    let first = segment_at(
        &key,
        0,
        vec![record(b"before-epoch", false)],
        None,
        observed_at,
        observed_at,
    );
    let epoch_transition = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 8, "runtime", 0).expect("cursor"),
        vec![record(b"after-epoch", false)],
        None,
        observed_at + 1,
        observed_at + 1,
    )
    .expect("segment")
    .sign(&key)
    .expect("signature");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            now,
        )
        .expect("first segment");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &epoch_transition.wire_bytes().expect("wire"),
            now,
        )
        .expect("permanent epoch gap");
    assert_eq!(runtime.snapshot.gaps[0].source_epoch, 7);
    assert_eq!(runtime.snapshot.gaps[0].end_unix_seconds, observed_at);
    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(observed_at, observed_at + 1, 10).expect("query"),
            LocalQueryMetadata::current_window(now),
        )
        .expect("query aggregates");
    let payload = response
        .records
        .iter()
        .find(|record| record.source_epoch == 8)
        .map(|record| serde_json::from_slice::<serde_json::Value>(&record.payload))
        .expect("epoch aggregate")
        .expect("aggregate JSON");
    assert_eq!(payload["complete"], false);
    runtime.snapshot.gaps.clear();
    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(observed_at, observed_at + 1, 10).expect("query"),
            LocalQueryMetadata::current_window(now),
        )
        .expect("query incomplete aggregate");
    assert_eq!(response.plan().completeness(), Completeness::Partial);
}

#[test]
fn dynamic_relay_hourly_attempt_gate_survives_repository_restart() {
    let first = tempfile::tempdir().expect("first repository");
    let mut sender = load(first.path());
    sender.snapshot.cluster_id = Some("cluster-a".to_owned());
    assert!(
        sender
            .begin_dynamic_relay_attempt(10_000)
            .expect("first relay attempt is due")
    );
    assert!(
        !sender
            .begin_dynamic_relay_attempt(10_001)
            .expect("relay attempt is rate limited")
    );
    let mut restored = load(first.path());
    assert!(
        !restored
            .begin_dynamic_relay_attempt(10_001)
            .expect("relay attempt stays rate limited after restart")
    );
}

#[test]
fn sqlite_restart_restores_continuous_acknowledgements_and_records() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let first_hash = first.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    let receipt = runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("accepted first segment");
    assert_eq!(receipt.acknowledgement.sequence, 0);

    let mut restored = load(temporary.path());
    let second = segment(&key, 1, vec![record(b"two", false)], Some(first_hash));
    let receipt = restored
        .receive_wire(
            "cluster-a",
            &identity,
            &second.wire_bytes().expect("wire"),
            12,
        )
        .expect("restored receiver accepts next segment");
    assert_eq!(receipt.acknowledgement.sequence, 1);
    let response = restored
        .query(
            "repository-a",
            HistoryQuery::new(11, 11, 10).expect("query"),
            LocalQueryMetadata::current_window(11),
        )
        .expect("query response");
    assert_eq!(response.plan.completeness(), Completeness::Complete);
    assert_eq!(response.plan.repository_id(), Some("repository-a"));
    assert_eq!(response.records.len(), 2);
}

#[test]
fn gaps_do_not_advance_the_persisted_acknowledgement() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let first_hash = first.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("first segment");
    let gap = segment(&key, 2, vec![record(b"three", false)], Some(first_hash));
    assert!(matches!(
        runtime.receive_wire("cluster-a", &identity, &gap.wire_bytes().expect("wire"), 12),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::SequenceGap {
                expected: 1,
                actual: 2,
            }
        ))
    ));
    let restored = load(temporary.path());
    assert_eq!(restored.snapshot.gaps[0].first_sequence, 1);
    assert_eq!(restored.snapshot.gaps[0].last_sequence, 1);
    assert_eq!(restored.snapshot.gaps[0].start_unix_seconds, 0);
    let response = restored
        .query(
            "repository-a",
            HistoryQuery::new(11, 11, 10).expect("query"),
            LocalQueryMetadata::current_window(11),
        )
        .expect("query response");
    assert_eq!(response.plan.watermarks()[0].sequence(), 0);
    assert_eq!(response.plan.gaps().len(), 1);
}

#[test]
fn authenticated_source_gap_allows_the_stream_to_resume_after_dropped_records() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let first_hash = first.segment_hash().expect("hash");
    let resumed = segment(&key, 2, vec![record(b"three", false)], Some(first_hash));
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("first segment");
    runtime
        .merge_replica_gaps(&[super::sync::RepositoryReplicaGap {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
            first_sequence: 1,
            last_sequence: 1,
            start_unix_seconds: 12,
            end_unix_seconds: 12,
            permanent: true,
        }])
        .expect("authenticated source gap");

    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &resumed.wire_bytes().expect("wire"),
            13,
        )
        .expect("resume after declared gap");
    assert_eq!(runtime.replication_summary().unwrap().segment_ids.len(), 2);
}

#[test]
fn repaired_segments_clear_gaps_only_after_the_continuous_chain_is_restored() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let first_hash = first.segment_hash().expect("hash");
    let second = segment(&key, 1, vec![record(b"two", false)], Some(first_hash));
    let second_hash = second.segment_hash().expect("hash");
    let third = segment(&key, 2, vec![record(b"three", false)], Some(second_hash));
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("first segment");
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &third.wire_bytes().expect("wire"),
            13,
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::SequenceGap {
                expected: 1,
                actual: 2,
            }
        ))
    ));
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &second.wire_bytes().expect("wire"),
            12,
        )
        .expect("repair the missing segment");
    let receipt = runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &third.wire_bytes().expect("wire"),
            13,
        )
        .expect("restore continuous acknowledgement");
    assert_eq!(receipt.acknowledgement.sequence, 2);
    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(11, 11, 10).expect("query"),
            LocalQueryMetadata::current_window(13),
        )
        .expect("query response");
    assert_eq!(response.plan.completeness(), Completeness::Complete);
    assert!(response.plan.gaps().is_empty());
}

#[test]
fn tombstones_and_unknown_schemas_are_preserved_without_resurrection_or_querying() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let tombstone = segment(&key, 0, vec![record(b"dead", true)], None);
    let tombstone_hash = tombstone.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &tombstone.wire_bytes().expect("wire"),
            11,
        )
        .expect("tombstone");
    let resurrection = segment(&key, 1, vec![record(b"dead", false)], Some(tombstone_hash));
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &resurrection.wire_bytes().expect("wire"),
            12,
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::ResurrectionPrevented
        ))
    ));
    let future = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 8, "runtime", 0).expect("cursor"),
        vec![SyncRecord::new(
            "subject-a",
            "node-a",
            "future.v99",
            99,
            b"future".to_vec(),
            b"raw".to_vec(),
            false,
        )],
        None,
        12,
        13,
    )
    .expect("future segment")
    .sign(&key)
    .expect("signature");
    let unknown_temporary = tempfile::tempdir().expect("temporary directory");
    let mut unknown_runtime = load(unknown_temporary.path());
    let receipt = unknown_runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &future.wire_bytes().expect("wire"),
            13,
        )
        .expect("unknown schema accepted");
    assert_eq!(receipt.unknown_schema_records, 1);
    let response = unknown_runtime
        .query(
            "repository-a",
            HistoryQuery::new(13, 13, 10).expect("query"),
            LocalQueryMetadata::current_window(13),
        )
        .expect("query response");
    assert_eq!(response.plan.completeness(), Completeness::LocalOnly);
    assert!(response.plan.coverage().is_some());
    assert!(!response.plan.gaps().is_empty());
    assert!(response.records.is_empty());
}

#[test]
fn expired_tombstones_allow_replacement_after_fresh_activity() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let tombstone = segment(&key, 0, vec![record(b"dead", true)], None);
    let tombstone_hash = tombstone.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &tombstone.wire_bytes().expect("wire"),
            100,
        )
        .expect("tombstone");
    let keep_fresh_at = 100 + TOMBSTONE_HORIZON_SECONDS - 1;
    let keep_fresh = segment(&key, 1, vec![record(b"other", false)], Some(tombstone_hash));
    let keep_fresh_hash = keep_fresh.segment_hash().expect("hash");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &keep_fresh.wire_bytes().expect("wire"),
            keep_fresh_at,
        )
        .expect("fresh activity");
    let replacement = segment(&key, 2, vec![record(b"dead", false)], Some(keep_fresh_hash));
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &replacement.wire_bytes().expect("wire"),
            keep_fresh_at + 1,
        )
        .expect("expired tombstone does not block a replacement record");
}

#[test]
fn tombstone_prefix_removes_persisted_historical_keyspace_before_blocking_resurrection() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let live = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 0).expect("traffic cursor"),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "traffic.v1",
            1,
            b"node-history:node:node-a:daily-traffic:2026-08-14".to_vec(),
            b"historical payload".to_vec(),
            false,
        )],
        None,
        10,
        10,
    )
    .expect("live historical segment")
    .sign(&key)
    .expect("sign live historical segment");
    let tombstone = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "tombstone", 0).expect("tombstone cursor"),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "traffic.v1",
            1,
            b"node-history:node:node-a:".to_vec(),
            b"deleted".to_vec(),
            true,
        )],
        None,
        11,
        11,
    )
    .expect("tombstone segment")
    .sign(&key)
    .expect("sign tombstone segment");
    let mut runtime = load(temporary.path());

    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &live.wire_bytes().expect("live wire"),
            10,
        )
        .expect("store historical record");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &tombstone.wire_bytes().expect("tombstone wire"),
            11,
        )
        .expect("accept tombstone");

    let records = runtime
        .storage
        .repository_history_records(None, None, None, 0, 8)
        .expect("read persisted records");
    assert!(
        records.is_empty(),
        "tombstones are retained in the ledger, not query rows"
    );
    let resurrect = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "traffic", 1).expect("next traffic cursor"),
        vec![SyncRecord::new(
            "node-a",
            "node-a",
            "traffic.v1",
            1,
            b"node-history:node:node-a:daily-traffic:2026-08-14".to_vec(),
            b"resurrected".to_vec(),
            false,
        )],
        Some(live.segment_hash().expect("live hash")),
        12,
        12,
    )
    .expect("resurrection segment")
    .sign(&key)
    .expect("sign resurrection segment");
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &resurrect.wire_bytes().expect("resurrection wire"),
            12,
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::ResurrectionPrevented
        ))
    ));
}

#[test]
fn node_tombstone_removes_peer_backfill_records_from_a_repository_import_stream() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let peer_key = SigningKey::from_bytes(&[13; 32]);
    let peer_identity = identity_for(&peer_key, "node-b");
    let imported = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "path_health-backfill-node-b", 0).expect("backfill cursor"),
        vec![SyncRecord::new(
            "node-b",
            "node-b",
            "path_health.v1",
            1,
            b"node-history:node:node-b:mesh:peer-c:2026-08-14T00:00:00Z".to_vec(),
            b"peer history".to_vec(),
            false,
        )],
        None,
        10,
        10,
    )
    .expect("peer backfill")
    .sign(&key)
    .expect("sign peer backfill");
    let tombstone = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-b", 7, "tombstone", 0).expect("tombstone cursor"),
        vec![SyncRecord::new(
            "node-b",
            "node-b",
            "path_health.v1",
            1,
            b"node-history:node:node-b:".to_vec(),
            b"deleted".to_vec(),
            true,
        )],
        None,
        11,
        11,
    )
    .expect("node tombstone")
    .sign(&peer_key)
    .expect("sign tombstone");
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &imported.wire_bytes().expect("backfill wire"),
            10,
        )
        .expect("persist peer backfill");
    runtime
        .receive_wire(
            "cluster-a",
            &peer_identity,
            &tombstone.wire_bytes().expect("tombstone wire"),
            11,
        )
        .expect("delete imported peer history");
    assert!(
        runtime
            .storage
            .repository_history_records(None, None, None, 0, 8)
            .expect("read records")
            .is_empty()
    );
}

#[test]
fn low_space_stops_history_writes_but_not_control_plane_operations() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let mut runtime = load(temporary.path());
    runtime
        .force_capacity_for_test(0, HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES - 1)
        .expect("capacity");
    assert_eq!(
        runtime.history_write_availability(),
        HistoryWriteAvailability::DegradedLowSpace
    );
    assert!(runtime.control_plane_permitted());
    let history = segment(&key, 0, vec![record(b"blocked", false)], None);
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &history.wire_bytes().expect("wire"),
            11,
        ),
        Err(RepositoryRuntimeError::WriteStopped(
            HistoryWriteAvailability::DegradedLowSpace
        ))
    ));
}
