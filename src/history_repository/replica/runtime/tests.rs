use super::*;
use crate::{
    history_sync::{CanonicalSegment, SyncRecord},
    state::history_repository::identity::{Ed25519PublicKey, RepositoryNodeId, X25519PublicKey},
};
use ed25519_dalek::SigningKey;

pub(super) fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[11; 32])
}
pub(super) fn identity(key: &SigningKey) -> RepositoryNodeIdentity {
    identity_for(key, "node-a")
}

pub(super) fn identity_for(key: &SigningKey, node_id: &str) -> RepositoryNodeIdentity {
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from(node_id.to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

pub(super) fn record(key: &[u8], tombstone: bool) -> SyncRecord {
    SyncRecord::new(
        "subject-a",
        "node-a",
        "runtime.v1",
        1,
        key.to_vec(),
        b"sample".to_vec(),
        tombstone,
    )
}

pub(super) fn traffic_record(key: &[u8]) -> SyncRecord {
    SyncRecord::new(
        "subject-a",
        "node-a",
        "traffic.v1",
        1,
        key.to_vec(),
        b"sample".to_vec(),
        false,
    )
}

pub(super) fn segment(
    signing_key: &SigningKey,
    sequence: u64,
    records: Vec<SyncRecord>,
    previous: Option<[u8; 32]>,
) -> SignedSegment {
    segment_at(signing_key, sequence, records, previous, 10, 11)
}

pub(super) fn segment_at(
    signing_key: &SigningKey,
    sequence: u64,
    records: Vec<SyncRecord>,
    previous: Option<[u8; 32]>,
    opened_at_unix_seconds: u64,
    closed_at_unix_seconds: u64,
) -> SignedSegment {
    CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
        records,
        previous,
        opened_at_unix_seconds,
        closed_at_unix_seconds,
    )
    .expect("segment")
    .sign(signing_key)
    .expect("signature")
}

pub(super) fn load(path: &std::path::Path) -> RepositoryReplicaRuntime {
    RepositoryReplicaRuntime::load(HistoryStorage::open(path)).expect("runtime")
}

#[test]
fn two_repositories_repair_a_partition_to_the_same_segment_set() {
    let first_repository = tempfile::tempdir().expect("first repository");
    let second_repository = tempfile::tempdir().expect("second repository");
    let key = signing_key();
    let identity = identity(&key);
    let segment = segment(&key, 0, vec![record(b"partitioned", false)], None);
    let wire = segment.wire_bytes().expect("wire");
    let ready = vec!["repo-a".to_owned(), "repo-b".to_owned()];
    let mut primary = load(first_repository.path());
    let mut standby = load(second_repository.path());
    primary
        .receive_wire_from_repository("cluster-a", &identity, &wire, 11, &ready, "repo-a")
        .expect("primary accepts source segment");

    let missing = standby
        .missing_segment_ids(&primary.replication_summary().expect("summary"), false)
        .expect("partition summary repair");
    let repair = primary
        .repair_batch(&missing)
        .expect("bounded repair batch");
    for segment in repair.segments {
        standby
            .receive_wire_from_repository(
                "cluster-a",
                &segment.identity,
                &segment.wire,
                12,
                &ready,
                "repo-b",
            )
            .expect("standby applies repaired segment");
    }
    assert!(
        standby
            .missing_segment_ids(&primary.replication_summary().expect("summary"), true)
            .expect("converged summary")
            .is_empty()
    );
}

#[test]
fn sqlite_replication_summary_keyset_pages_every_segment_beyond_the_first_256() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let ready = vec!["repository-a".to_owned()];
    let mut runtime = load(temporary.path());
    let mut previous = None;
    for sequence in 0..257_u64 {
        let signed = segment_at(
            &key,
            sequence,
            vec![record(format!("segment-{sequence}").as_bytes(), false)],
            previous,
            sequence,
            sequence,
        );
        previous = Some(signed.segment_hash().expect("segment hash"));
        runtime
            .receive_wire_from_repository(
                "cluster-a",
                &identity,
                &signed.wire_bytes().expect("segment wire"),
                sequence,
                &ready,
                "repository-a",
            )
            .expect("store SQLite segment");
    }

    let first = runtime.replication_summary().expect("first summary page");
    assert_eq!(first.segment_ids.len(), 256);
    let cursor = first.next_segment_id.expect("keyset cursor");
    let second = runtime
        .replication_summary_after(Some(&cursor))
        .expect("second summary page");
    assert_eq!(second.segment_ids.len(), 1);
    assert!(second.next_segment_id.is_none());
    assert!(!first.segment_ids.contains(&second.segment_ids[0]));
    assert!(
        runtime
            .repair_batch(&second.segment_ids)
            .expect("repair a later SQLite segment")
            .segments
            .len()
            == 1
    );
}

#[test]
fn daily_deep_verification_summarizes_source_stream_ranges() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment_at(&key, 0, vec![record(b"one", false)], None, 100, 100);
    let second = segment_at(
        &key,
        1,
        vec![record(b"two", false)],
        Some(first.segment_hash().expect("first hash")),
        101,
        101,
    );
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("first wire"),
            101,
        )
        .expect("first segment");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &second.wire_bytes().expect("second wire"),
            102,
        )
        .expect("second segment");

    let summary = serde_json::to_value(runtime.replication_summary().expect("summary"))
        .expect("serialized summary");
    let partitions = summary["partitions"].as_array().expect("partitions");
    assert_eq!(partitions.len(), 1);
    assert_eq!(partitions[0]["first_sequence"], 0);
    assert_eq!(partitions[0]["last_sequence"], 1);
    assert_eq!(partitions[0]["record_count"], 2);
}

#[test]
fn tombstone_expiry_waits_for_every_ready_repository_after_restart() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let ready = vec!["repo-a".to_owned(), "repo-b".to_owned()];
    let tombstone = segment(&key, 0, vec![record(b"dead", true)], None);
    let tombstone_hash = tombstone.segment_hash().expect("hash");
    let mut runtime = load(temporary.path());
    let receipt = runtime
        .receive_wire_from_repository(
            "cluster-a",
            &identity,
            &tombstone.wire_bytes().expect("wire"),
            100,
            &ready,
            "repo-a",
        )
        .expect("tombstone");
    let mut remote_acknowledgement = receipt.tombstone_acknowledgements()[0].clone();
    remote_acknowledgement.repository_id = "repo-b".to_owned();

    let mut restored = load(temporary.path());
    let keep = segment(&key, 1, vec![record(b"other", false)], Some(tombstone_hash));
    let keep_hash = keep.segment_hash().expect("hash");
    restored
        .receive_wire_from_repository(
            "cluster-a",
            &identity,
            &keep.wire_bytes().expect("wire"),
            100 + TOMBSTONE_HORIZON_SECONDS,
            &ready,
            "repo-a",
        )
        .expect("unacknowledged tombstone remains durable");
    restored
        .acknowledge_tombstones(&[remote_acknowledgement])
        .expect("second ready repository acknowledgement");
    let replacement = segment(&key, 2, vec![record(b"dead", false)], Some(keep_hash));
    restored
        .receive_wire_from_repository(
            "cluster-a",
            &identity,
            &replacement.wire_bytes().expect("wire"),
            100 + TOMBSTONE_HORIZON_SECONDS + 1,
            &ready,
            "repo-a",
        )
        .expect("all-ready acknowledgement permits expiry and replacement");
}

#[test]
fn repository_retention_aggregates_old_ip_history_without_raw_identifiers() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let observed_at = 1_000_u64;
    let now = observed_at + 7 * 24 * 60 * 60 + 2;
    let ip = b"192.0.2.99";
    let record = SyncRecord::new(
        "subject-a",
        "node-a",
        "ip_usage.v1",
        1,
        ip.to_vec(),
        ip.to_vec(),
        false,
    );
    let segment = segment_at(&key, 0, vec![record], None, observed_at, observed_at);
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = false;
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment.wire_bytes().expect("wire"),
            now,
        )
        .expect("repository accepts old IP history");
    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(observed_at, observed_at, 10).expect("query"),
            LocalQueryMetadata::current_window(now),
        )
        .expect("query aggregate");
    assert_eq!(response.records.len(), 1);
    assert_ne!(response.records[0].record_key, ip);
    let payload = String::from_utf8(response.records[0].payload.clone()).expect("JSON aggregate");
    assert!(payload.contains("anonymized_identifier"));
    assert!(!payload.contains("192.0.2.99"));
    let payload: serde_json::Value = serde_json::from_str(&payload).expect("aggregate JSON");
    assert_eq!(payload["bucket_start_unix_seconds"], 900);
    assert_eq!(payload["bucket_end_unix_seconds"], 1_199);
    let coverage = response.plan().coverage().expect("repository coverage");
    assert_eq!(coverage.observed().start_unix_seconds(), 900);
    assert_eq!(coverage.observed().end_unix_seconds(), 1_199);
    assert_eq!(coverage.received().start_unix_seconds(), now);
    assert_eq!(coverage.received().end_unix_seconds(), now);
}

#[test]
fn legacy_aggregate_payload_keeps_its_original_query_coverage_after_restart() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let observed_at = 1_000_u64;
    let now = observed_at + 7 * 24 * 60 * 60 + 2;
    let record = SyncRecord::new(
        "subject-a",
        "node-a",
        "ip_usage.v1",
        1,
        b"192.0.2.99".to_vec(),
        b"192.0.2.99".to_vec(),
        false,
    );
    let segment = segment_at(&key, 0, vec![record], None, observed_at, observed_at);
    let mut runtime = load(temporary.path());
    runtime.snapshot.external_history = false;
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment.wire_bytes().expect("wire"),
            now,
        )
        .expect("repository accepts old IP history");
    let mut payload: serde_json::Value =
        serde_json::from_slice(&runtime.snapshot.records[0].payload).expect("aggregate JSON");
    payload
        .as_object_mut()
        .expect("aggregate object")
        .remove("bucket_start_unix_seconds");
    payload
        .as_object_mut()
        .expect("aggregate object")
        .remove("bucket_end_unix_seconds");
    runtime.snapshot.records[0].payload = serde_json::to_vec(&payload).expect("legacy payload");
    runtime
        .persist_control_state()
        .expect("persist legacy payload");

    let restored = load(temporary.path());
    let response = restored
        .query(
            "repository-a",
            HistoryQuery::new(observed_at, observed_at, 10).expect("query"),
            LocalQueryMetadata::current_window(now),
        )
        .expect("query legacy aggregate");
    assert_eq!(response.records.len(), 1);
    let coverage = response.plan().coverage().expect("repository coverage");
    assert_eq!(coverage.observed().start_unix_seconds(), observed_at);
    assert_eq!(coverage.observed().end_unix_seconds(), observed_at);
}

#[test]
fn repository_retention_keeps_distinct_anonymized_ip_aggregates() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let observed_at = 1_000_u64;
    let now = observed_at + 7 * 24 * 60 * 60 + 2;
    let ip_records = [b"192.0.2.10".to_vec(), b"192.0.2.11".to_vec()]
        .into_iter()
        .map(|ip| {
            SyncRecord::new(
                "subject-a",
                "node-a",
                "ip_usage.v1",
                1,
                ip.clone(),
                ip,
                false,
            )
        })
        .collect();
    let segment = segment_at(&key, 0, ip_records, None, observed_at, observed_at);
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment.wire_bytes().expect("wire"),
            now,
        )
        .expect("repository accepts IP history");
    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(observed_at, observed_at, 10).expect("query"),
            LocalQueryMetadata::current_window(now),
        )
        .expect("query aggregates");
    assert_eq!(response.records.len(), 2);
    let identifiers = response
        .records
        .iter()
        .map(|record| {
            serde_json::from_slice::<serde_json::Value>(&record.payload)
                .expect("aggregate JSON")["anonymized_identifier"]
                .as_str()
                .expect("anonymized identifier")
                .to_owned()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(identifiers.len(), 2);
    let keys = response
        .records
        .iter()
        .map(|record| record.record_key.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(keys.len(), 2);
}

#[test]
fn replica_repair_propagates_canonical_gap_metadata() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let segment = segment(&key, 0, vec![record(b"one", false)], None);
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &segment.wire_bytes().expect("wire"),
            11,
        )
        .expect("segment");
    let mut remote = runtime.replication_summary().expect("summary");
    remote.gaps.push(super::sync::RepositoryReplicaGap {
        source_node_id: "node-a".to_owned(),
        source_epoch: 7,
        stream: "runtime".to_owned(),
        first_sequence: 0,
        last_sequence: 0,
        start_unix_seconds: 10,
        end_unix_seconds: 11,
        permanent: true,
    });
    assert!(
        runtime
            .requires_repair(&remote, true)
            .expect("gap metadata needs repair")
    );
    assert!(
        runtime
            .missing_segment_ids(&remote, true)
            .expect("gap-only repair has no segments")
            .is_empty()
    );
    runtime
        .merge_replica_gaps(&remote.gaps)
        .expect("merge remote gap metadata");
    assert!(
        !runtime
            .requires_repair(&remote, true)
            .expect("gap metadata converged")
    );
}

#[test]
fn rendezvous_collector_failover_is_persisted_after_three_primary_cycles() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let ready = vec![
        "repository-a".to_owned(),
        "repository-b".to_owned(),
        "repository-c".to_owned(),
    ];
    let assignment = super::super::rendezvous_collectors("source-a", &ready).expect("assignment");
    let standby = assignment.standby().expect("standby").to_owned();
    let mut runtime = load(temporary.path());
    assert!(
        runtime
            .collects_source("source-a", &ready, assignment.primary())
            .expect("primary collector")
    );
    runtime
        .snapshot
        .source_last_received_unix_seconds
        .insert("source-a".to_owned(), 100);
    for now in [400, 700, 1_000] {
        runtime
            .record_stale_collection_cycles(
                now,
                &ready,
                assignment.primary(),
                &["source-a".to_owned()],
            )
            .expect("record stale primary cycle");
    }
    runtime
        .record_collection_cycle("source-a", &ready, &standby, true)
        .expect("standby success does not clear primary failure state");
    let restored = load(temporary.path());
    assert!(
        restored
            .collects_source("source-a", &ready, &standby)
            .expect("standby collector")
    );
}

#[test]
fn repository_retention_preserves_aggregate_counts_across_prune_cycles() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let observed_at = 1_000_u64;
    let now = observed_at + 7 * 24 * 60 * 60 + 2;
    let first = segment_at(
        &key,
        0,
        vec![record(b"first", false)],
        None,
        observed_at,
        observed_at,
    );
    let second = segment_at(
        &key,
        1,
        vec![record(b"second", false)],
        Some(first.segment_hash().expect("hash")),
        observed_at + 1,
        observed_at + 1,
    );
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            now,
        )
        .expect("first aggregate");
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &second.wire_bytes().expect("wire"),
            now,
        )
        .expect("second aggregate");
    let response = runtime
        .query(
            "repository-a",
            HistoryQuery::new(observed_at, observed_at + 1, 10).expect("query"),
            LocalQueryMetadata::current_window(now),
        )
        .expect("query aggregate");
    assert_eq!(response.records.len(), 1);
    let payload: serde_json::Value =
        serde_json::from_slice(&response.records[0].payload).expect("aggregate JSON");
    assert_eq!(payload["record_count"], 2);
    assert_eq!(payload["first_sequence"], 0);
    assert_eq!(payload["last_sequence"], 1);
}

#[test]
fn quiet_sqlite_repository_compacts_at_the_exact_tiered_retention_boundaries() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let policy = super::super::RepositoryRetentionPolicy::default();
    let now = policy.max_age_seconds().saturating_add(10_000);
    let day = 24 * 60 * 60;
    let timestamps = [
        now.saturating_sub(policy.max_age_seconds())
            .saturating_sub(1),
        now.saturating_sub(
            policy.minute_retention_seconds() + policy.five_minute_retention_seconds() + 1,
        ),
        now.saturating_sub(policy.minute_retention_seconds() + 1),
        now.saturating_sub(policy.minute_retention_seconds()),
    ];
    let mut runtime = load(temporary.path());
    let mut previous = None;
    for (sequence, observed_at) in timestamps.into_iter().enumerate() {
        let signed = segment_at(
            &key,
            u64::try_from(sequence).expect("sequence"),
            vec![record(format!("boundary-{sequence}").as_bytes(), false)],
            previous,
            observed_at,
            observed_at,
        );
        previous = Some(signed.segment_hash().expect("segment hash"));
        runtime
            .receive_wire(
                "cluster-a",
                &identity,
                &signed.wire_bytes().expect("wire"),
                observed_at,
            )
            .expect("store history before quiet period");
    }

    runtime
        .prepare_for_replication(now)
        .expect("periodic idle compaction");
    let records = runtime
        .sqlite_records(None, None, None, 0, 16)
        .expect("read compacted SQLite rows");
    assert_eq!(records.len(), 3, "older than two years expires");
    let mut resolutions = records
        .iter()
        .filter_map(|record| {
            serde_json::from_slice::<serde_json::Value>(&record.payload)
                .ok()
                .and_then(|payload| payload["resolution"].as_str().map(str::to_owned))
        })
        .collect::<Vec<_>>();
    resolutions.sort();
    assert_eq!(resolutions, vec!["five_minutes", "hour"]);
    assert!(
        records
            .iter()
            .any(|record| record.observed_at_unix_seconds == now.saturating_sub(7 * day))
    );
}

#[test]
fn sqlite_retention_keyset_advances_past_more_than_one_compaction_page() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let first_observed = 1_000_u64;
    let interval = 5 * 60;
    let count = RETENTION_COMPACTION_PAGE_SIZE + 1;
    let now = first_observed
        .saturating_add(u64::try_from(count).expect("count") * interval)
        .saturating_add(policy.minute_retention_seconds())
        .saturating_add(1);
    let mut runtime = load(temporary.path());
    let rows = (0..u64::try_from(count).expect("count"))
        .map(|sequence| {
            let observed_at = first_observed.saturating_add(sequence * interval);
            StoredRecord {
                observed_at_unix_seconds: observed_at,
                received_at_unix_seconds: now,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "runtime".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "runtime.v1".to_owned(),
                schema_version: 1,
                record_key: format!("bucket-{sequence}").into_bytes(),
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
        .expect("seed more than one retention page");
    runtime
        .prepare_for_replication(now)
        .expect("compact first page");
    assert!(runtime.snapshot.retention_compaction_cursor.is_some());
    runtime
        .prepare_for_replication(now)
        .expect("advance the second compaction page");
    assert!(runtime.snapshot.retention_compaction_cursor.is_none());
}

#[test]
fn sqlite_retention_does_not_split_a_bucket_at_a_keyset_page_boundary() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let first_observed = 1_000_u64;
    let count = RETENTION_COMPACTION_PAGE_SIZE + 1;
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
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("traffic-{sequence}").into_bytes(),
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
        .expect("seed boundary page");
    runtime
        .prepare_for_replication(now)
        .expect("compact the first bounded page");
    while runtime.snapshot.retention_compaction_cursor.is_some() {
        runtime
            .prepare_for_replication(now)
            .expect("compact a bounded page");
    }
    let records = runtime
        .sqlite_records(None, None, None, 0, count)
        .expect("read compacted records");
    let mut per_bucket = std::collections::BTreeMap::<u64, usize>::new();
    for record in records {
        let payload: serde_json::Value =
            serde_json::from_slice(&record.payload).expect("aggregated payload");
        let bucket = payload["bucket_start_unix_seconds"]
            .as_u64()
            .expect("bucket start");
        *per_bucket.entry(bucket).or_default() += 1;
    }
    assert!(
        per_bucket.values().all(|count| *count == 1),
        "a five-minute aggregate is never split by a SQLite keyset page"
    );
}

#[test]
fn sqlite_retention_progresses_when_one_bucket_exceeds_the_lookahead() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let policy = super::super::RepositoryRetentionPolicy::default();
    let now = 10_000_u64
        .saturating_add(policy.minute_retention_seconds())
        .saturating_add(1);
    let count = RETENTION_COMPACTION_PAGE_SIZE + RETENTION_COMPACTION_BUCKET_LOOKAHEAD + 32;
    let runtime = load(temporary.path());
    let rows = (0..u64::try_from(count).expect("count"))
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: 10_000,
                received_at_unix_seconds: now,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "traffic".to_owned(),
                sequence,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "traffic.v1".to_owned(),
                schema_version: 1,
                record_key: format!("dense-{sequence}").into_bytes(),
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
    let mut runtime = runtime;
    runtime
        .prepare_for_replication(now)
        .expect("compact first continuation page");
    runtime
        .prepare_for_replication(now)
        .expect("merge continuation page");
    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("record count"),
        1,
        "dense bucket is folded into one bounded aggregate"
    );
}
