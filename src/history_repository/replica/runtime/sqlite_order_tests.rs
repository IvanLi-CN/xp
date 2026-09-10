use ed25519_dalek::SigningKey;

#[cfg(target_os = "linux")]
use std::{fs, process::Command};

use super::{
    RepositoryPartitionSummary, RepositoryReplicaRuntime, RepositoryRuntimeError, StoredRecord,
    StoredSegment,
};
use crate::{
    history_sync::{CanonicalSegment, Cursor, SignedSegment, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
        replica::RepositoryRetentionPolicy,
    },
    state::history_storage::{
        Backend, RepositoryHistoryCompactionCursor, RepositoryHistoryRecordRow,
        RepositoryHistorySegmentRow,
    },
};

use super::tests::load;

#[test]
fn malformed_sqlite_summary_row_does_not_block_replication_preparation() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = true;
    runtime.snapshot.legacy_segment_cursor_index_complete = true;
    runtime
        .storage
        .upsert_repository_history_records(&[RepositoryHistoryRecordRow {
            source_node_id: "node-a".to_owned(),
            source_epoch: 7,
            stream: "runtime".to_owned(),
            sequence: 0,
            subject_node_id: "subject-a".to_owned(),
            observer_node_id: "node-a".to_owned(),
            schema_id: "runtime.v1".to_owned(),
            schema_version: 1,
            record_key: b"key".to_vec(),
            tombstone: false,
            observed_start_unix_seconds: 10,
            observed_end_unix_seconds: 10,
            received_at_unix_seconds: 10,
            aggregate_complete: Some(true),
            aggregate_start_unix_seconds: None,
            aggregate_end_unix_seconds: None,
            payload: b"not a stored record".to_vec(),
        }])
        .expect("seed malformed payload");
    runtime.snapshot.partition_summaries_complete = false;

    runtime
        .prepare_for_replication(100)
        .expect("malformed summary row is deferred");
    assert!(!runtime.partition_summaries_ready());
    assert!(runtime.replication_summary().is_ok());
}

#[test]
fn incomplete_sqlite_partition_summary_does_not_trigger_tiered_backfill() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = true;
    runtime.snapshot.legacy_segment_cursor_index_complete = true;
    runtime.snapshot.partition_summaries_complete = false;
    let mut remote = runtime
        .replication_summary_after(None, true)
        .expect("remote deep summary");
    remote.partitions_included = true;
    remote.partitions = vec![RepositoryPartitionSummary {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        partition: 0,
        first_sequence: 0,
        last_sequence: 0,
        hash: [9; 32],
        record_count: 1,
    }];

    assert!(
        !runtime
            .requires_repair(&remote, true)
            .expect("incomplete local cache is not a mismatch")
    );
    assert!(
        runtime
            .retained_partitions_converged(&remote)
            .expect("incomplete local cache defers comparison")
    );
}

fn identity(signing_key: &SigningKey) -> RepositoryNodeIdentity {
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

#[test]
fn sqlite_summary_keeps_tombstones_first_across_keyset_pages() {
    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let repository_identity = identity(&signing_key);
    let signed = |stream: &str, tombstone: bool| {
        CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 1, stream, 0).expect("cursor"),
            vec![SyncRecord::new(
                "subject-a",
                "node-a",
                "runtime.v1",
                1,
                stream.as_bytes().to_vec(),
                b"payload".to_vec(),
                tombstone,
            )],
            None,
            10,
            11,
        )
        .expect("segment")
        .sign(&signing_key)
        .expect("signed segment")
        .wire_bytes()
        .expect("wire")
    };
    let ordinary = StoredSegment {
        id: "000-ordinary".to_owned(),
        closed_at_unix_seconds: 10,
        identity: repository_identity.clone(),
        wire: signed("ordinary", false),
    };
    let tombstone_wire = signed("tombstone", true);
    let mut segments = vec![ordinary];
    segments.extend((0..256).map(|index| StoredSegment {
        id: format!("tombstone-{index:03}"),
        closed_at_unix_seconds: 11,
        identity: repository_identity.clone(),
        wire: tombstone_wire.clone(),
    }));
    let rows = segments
        .iter()
        .map(StoredSegment::sqlite_row)
        .collect::<Result<Vec<_>, _>>()
        .expect("SQLite rows");
    let runtime =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    runtime
        .storage
        .upsert_repository_history_segments(&rows)
        .expect("store interleaved segment IDs");

    let first = runtime.replication_summary().expect("first summary page");
    assert_eq!(first.segment_ids.len(), 256);
    assert!(
        first
            .segment_ids
            .iter()
            .all(|id| id.starts_with("tombstone-"))
    );
    let cursor = first.next_segment_id.expect("tombstone page cursor");
    assert!(cursor.starts_with("t:"));

    let second = runtime
        .replication_summary_after(Some(&cursor), false)
        .expect("ordinary summary page");
    assert_eq!(second.segment_ids, ["000-ordinary"]);
    assert!(second.next_segment_id.is_none());
}

#[test]
fn sqlite_partition_summary_cache_survives_restart_and_invalidates_on_late_record() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = true;
    runtime.snapshot.legacy_segment_cursor_index_complete = true;
    let rows = (0..2_u64)
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: 10 + sequence,
                received_at_unix_seconds: 10 + sequence,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "runtime".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "runtime.v1".to_owned(),
                schema_version: 1,
                record_key: sequence.to_be_bytes().to_vec(),
                payload: format!("payload-{sequence}").into_bytes(),
                tombstone: false,
            }
            .sqlite_row()
            .expect("SQLite history row")
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("seed history rows");
    runtime.snapshot.partition_summaries_complete = false;
    runtime
        .prepare_for_replication(100)
        .expect("rebuild history page");
    runtime
        .prepare_for_replication(100)
        .expect("finish history summary rebuild");
    assert!(runtime.partition_summaries_ready());
    runtime
        .persist_control_state()
        .expect("persist cache state");

    let mut restored = load(temporary.path());
    assert!(restored.partition_summaries_ready());
    let summary = restored
        .replication_summary_after(None, true)
        .expect("restored deep summary");
    assert!(summary.partitions_included);
    assert_eq!(summary.partitions[0].record_count, 2);

    let late = StoredRecord {
        observed_at_unix_seconds: 9,
        received_at_unix_seconds: 9,
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        sequence: 99,
        subject_node_id: "subject-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "runtime.v1".to_owned(),
        schema_version: 1,
        record_key: b"late".to_vec(),
        payload: b"late-payload".to_vec(),
        tombstone: false,
    };
    restored
        .update_partition_summary_for_record(&late)
        .expect("invalidate stale cache");
    assert!(!restored.partition_summaries_ready());
    let pending = restored
        .replication_summary_after(None, true)
        .expect("summary while rebuilding");
    assert!(!pending.partitions_included);
}

#[test]
fn sqlite_retention_persists_summary_invalidation_before_committed_rewrite() {
    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let storage = HistoryStorage::open(temporary.path());
    let mut runtime = RepositoryReplicaRuntime::load(storage.clone()).expect("runtime");
    runtime.snapshot.external_history = true;
    runtime.snapshot.legacy_segment_cursor_index_complete = true;
    let row = StoredRecord {
        observed_at_unix_seconds: 10,
        received_at_unix_seconds: 10,
        source_node_id: "node-a".to_owned(),
        source_epoch: 1,
        stream: "runtime".to_owned(),
        sequence: 0,
        subject_node_id: "subject-a".to_owned(),
        observer_node_id: "node-a".to_owned(),
        schema_id: "runtime.v1".to_owned(),
        schema_version: 1,
        record_key: b"key".to_vec(),
        payload: b"payload".to_vec(),
        tombstone: false,
    };
    let row = row.sqlite_row().expect("SQLite row");
    storage
        .upsert_repository_history_records(std::slice::from_ref(&row))
        .expect("seed history row");
    runtime.snapshot.partition_summaries = vec![RepositoryPartitionSummary {
        source_node_id: "node-a".to_owned(),
        source_epoch: 1,
        stream: "runtime".to_owned(),
        partition: 0,
        first_sequence: 0,
        last_sequence: 0,
        hash: [7; 32],
        record_count: 1,
    }];
    runtime.snapshot.partition_summary_cursor = Some(RepositoryHistoryCompactionCursor::from(&row));
    runtime.snapshot.partition_summaries_complete = true;
    runtime
        .persist_control_state()
        .expect("persist ready cache");

    storage.set_history_rewrite_maintenance_failure_for_test(true);
    let now = 10 + RepositoryRetentionPolicy::default().minute_retention_seconds() + 2;
    assert!(runtime.prune_sqlite_retention(now).is_err());
    assert!(!runtime.partition_summaries_ready());

    drop(runtime);
    let restored = RepositoryReplicaRuntime::load(storage).expect("reload runtime");
    assert!(
        !restored.partition_summaries_ready(),
        "a committed rewrite must never restart with a stale complete cache"
    );
}

#[test]
fn sqlite_summary_reads_metadata_only() {
    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let runtime =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    let rows = (0..257_u64)
        .map(|sequence| RepositoryHistorySegmentRow {
            id: format!("segment-{sequence:03}"),
            closed_at_unix_seconds: sequence,
            contains_tombstone: false,
            source_node_id: "node-a".to_owned(),
            source_epoch: 1,
            stream: "runtime".to_owned(),
            first_sequence: sequence,
            payload: b"not-json".to_vec(),
        })
        .collect::<Vec<_>>();
    runtime
        .storage
        .upsert_repository_history_segments(&rows)
        .expect("store metadata rows");

    let summary = runtime
        .replication_summary_after(None, false)
        .expect("summary must not decode segment payloads");
    assert_eq!(summary.segment_ids.len(), 256);
    assert_eq!(summary.next_segment_id.as_deref(), Some("r:segment-255"));
}

#[cfg(target_os = "linux")]
const SUMMARY_RESOURCE_BENCHMARK_CHILD: &str = "XP_REPOSITORY_SUMMARY_RESOURCE_BENCHMARK_CHILD";

#[cfg(target_os = "linux")]
const SUMMARY_RESOURCE_BENCHMARK_TEST: &str = concat!(
    "state::history_repository::replica::runtime::sqlite_order_tests::",
    "repository_summary_resource_budget"
);

#[cfg(target_os = "linux")]
fn process_pss_bytes() -> u64 {
    fs::read_to_string("/proc/self/smaps_rollup")
        .expect("read process PSS")
        .lines()
        .find_map(|line| {
            line.strip_prefix("Pss:")?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .expect("parse process PSS")
        .saturating_mul(1024)
}

#[cfg(target_os = "linux")]
#[test]
fn repository_summary_resource_budget() {
    if std::env::var_os(SUMMARY_RESOURCE_BENCHMARK_CHILD).is_none() {
        let output = Command::new(std::env::current_exe().expect("current test executable"))
            .args(["--exact", SUMMARY_RESOURCE_BENCHMARK_TEST, "--nocapture"])
            .env(SUMMARY_RESOURCE_BENCHMARK_CHILD, "1")
            .output()
            .expect("run isolated repository summary resource benchmark");
        assert!(
            output.status.success(),
            "isolated repository summary resource benchmark failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let storage = HistoryStorage::open(temporary.path());
    drop(storage);
    let connection = rusqlite::Connection::open(temporary.path().join("history.sqlite3"))
        .expect("open history database");
    let transaction = connection
        .unchecked_transaction()
        .expect("begin segment backlog transaction");
    let payload = vec![0_u8; 256 * 1024];
    for sequence in 0..257_u64 {
        transaction
            .execute(
                "INSERT INTO repository_history_segments
                     (id, closed_at, contains_tombstone, source_node_id, source_epoch,
                      stream, first_sequence, payload)
                 VALUES (?1, ?2, 0, 'node-a', 1, 'runtime', ?2, ?3)",
                rusqlite::params![
                    format!("resource-segment-{sequence:03}"),
                    sequence,
                    &payload
                ],
            )
            .expect("insert repository segment");
    }
    transaction.commit().expect("commit segment backlog");
    drop(connection);

    let runtime =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    for _ in 0..3 {
        runtime
            .replication_summary_after(None, false)
            .expect("warm repository summary");
    }
    let baseline_pss = process_pss_bytes();
    let mut max_pss = baseline_pss;
    for _ in 0..5 {
        runtime
            .replication_summary_after(None, false)
            .expect("read bounded repository summary");
        max_pss = max_pss.max(process_pss_bytes());
    }
    println!("repository_summary_resource max_pss_bytes={max_pss}");
    assert!(
        max_pss < 32 * 1024 * 1024,
        "repository summary PSS {max_pss} is not below 32 MiB"
    );
}

#[test]
fn sqlite_summary_orders_repair_candidates_by_signed_cursor() {
    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let repository_identity = identity(&signing_key);
    let ready = vec!["repository-a".to_owned(), "repository-b".to_owned()];
    let mut primary =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    let mut previous_hash = None;

    for sequence in 0_u64..66 {
        let signed = CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                format!("runtime:{sequence}").into_bytes(),
                format!("sample:{sequence}").into_bytes(),
                false,
            )],
            previous_hash,
            100 + sequence,
            100 + sequence,
        )
        .expect("canonical segment")
        .sign(&signing_key)
        .expect("signed segment");
        previous_hash = Some(signed.segment_hash().expect("segment hash"));
        primary
            .receive_wire_from_repository(
                "cluster-a",
                &repository_identity,
                &signed.wire_bytes().expect("wire"),
                100 + sequence,
                &ready,
                "repository-a",
            )
            .expect("primary accepts ordered source segment");
    }

    let summary = primary.replication_summary().expect("SQLite summary");
    let missing = summary.segment_ids[..64].to_vec();
    let repair = primary.repair_batch(&missing).expect("repair batch");
    let sequences = repair
        .segments
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("repair wire")
                .canonical()
                .first_cursor()
                .sequence()
        })
        .collect::<Vec<_>>();
    assert_eq!(sequences, (0_u64..64).collect::<Vec<_>>());

    let standby_temporary = tempfile::tempdir().expect("standby SQLite temporary directory");
    let mut standby =
        RepositoryReplicaRuntime::load(HistoryStorage::open(standby_temporary.path()))
            .expect("standby runtime");
    for segment in repair.segments {
        standby
            .receive_wire_from_repository(
                "cluster-a",
                &segment.identity,
                &segment.wire,
                200,
                &ready,
                "repository-b",
            )
            .expect("SQLite repair segments must be accepted in source cursor order");
    }
}

#[test]
fn sqlite_load_defers_legacy_segment_cursor_index_migration() {
    let temporary = tempfile::tempdir().expect("SQLite temporary directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let repository_identity = identity(&signing_key);
    let ready = vec!["repository-a".to_owned()];
    let mut runtime =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("runtime");
    let mut previous_hash = None;
    for sequence in 0_u64..2 {
        let signed = CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
            vec![SyncRecord::new(
                "node-a",
                "node-a",
                "runtime.v1",
                1,
                format!("runtime:{sequence}").into_bytes(),
                format!("sample:{sequence}").into_bytes(),
                false,
            )],
            previous_hash,
            100,
            100,
        )
        .expect("canonical segment")
        .sign(&signing_key)
        .expect("signed segment");
        previous_hash = Some(signed.segment_hash().expect("segment hash"));
        runtime
            .receive_wire_from_repository(
                "cluster-a",
                &repository_identity,
                &signed.wire_bytes().expect("wire"),
                100,
                &ready,
                "repository-a",
            )
            .expect("store segment");
    }
    {
        let mut backend = runtime.storage.lock_backend();
        let Backend::Sqlite(connection) = &mut *backend else {
            panic!("test requires SQLite storage");
        };
        connection
            .execute(
                "UPDATE repository_history_segments
                 SET source_node_id = '', source_epoch = 0, stream = '', first_sequence = 0",
                [],
            )
            .expect("erase legacy cursor index");
    }
    runtime.snapshot.legacy_segment_cursor_index_after_id = None;
    runtime.snapshot.legacy_segment_cursor_index_complete = false;
    runtime
        .persist_control_state()
        .expect("checkpoint legacy state");
    drop(runtime);

    let mut restored =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("restore");
    assert!(matches!(
        restored.replication_summary(),
        Err(RepositoryRuntimeError::LegacySegmentCursorIndexPending)
    ));
    assert!(matches!(
        restored.repair_batch(&[]),
        Err(RepositoryRuntimeError::LegacySegmentCursorIndexPending)
    ));
    assert!(matches!(
        restored.relay_batch("repository-b"),
        Err(RepositoryRuntimeError::LegacySegmentCursorIndexPending)
    ));
    assert_eq!(
        restored
            .storage
            .repository_history_segments_missing_cursor_index(None, 3)
            .expect("inspect cursor index")
            .len(),
        2
    );
    assert!(
        !restored
            .migrate_legacy_segment_cursor_index_page(1)
            .expect("migrate one bounded page")
    );
    drop(restored);

    let mut restored =
        RepositoryReplicaRuntime::load(HistoryStorage::open(temporary.path())).expect("resume");
    assert_eq!(
        restored
            .storage
            .repository_history_segments_missing_cursor_index(None, 3)
            .expect("inspect resumed cursor index")
            .len(),
        1
    );
    assert!(
        !restored
            .migrate_legacy_segment_cursor_index_page(1)
            .expect("migrate resumed bounded page")
    );
    assert!(
        restored
            .storage
            .repository_history_segments_missing_cursor_index(None, 3)
            .expect("inspect cursor index")
            .is_empty()
    );
    assert!(
        restored
            .migrate_legacy_segment_cursor_index_page(1)
            .expect("complete migration")
    );
    let summary = restored.replication_summary().expect("SQLite summary");
    let repair = restored
        .repair_batch(&summary.segment_ids)
        .expect("repair batch");
    let sequences = repair
        .segments
        .iter()
        .map(|segment| {
            SignedSegment::from_wire(&segment.wire)
                .expect("repair wire")
                .canonical()
                .first_cursor()
                .sequence()
        })
        .collect::<Vec<_>>();
    assert_eq!(sequences, vec![0, 1]);
}
