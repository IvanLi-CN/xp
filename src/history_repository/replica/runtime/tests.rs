use ed25519_dalek::SigningKey;

use super::*;
use crate::{
    history_sync::{CanonicalSegment, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        control::{
            DEFAULT_HISTORY_REPOSITORY_QUOTA_BYTES, HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES,
            HistoryWriteAvailability,
        },
        identity::{Ed25519PublicKey, RepositoryNodeId, X25519PublicKey},
        query::Completeness,
    },
};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[11; 32])
}

fn identity(key: &SigningKey) -> RepositoryNodeIdentity {
    identity_for(key, "node-a")
}

fn identity_for(key: &SigningKey, node_id: &str) -> RepositoryNodeIdentity {
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from(node_id.to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity")
}

fn record(key: &[u8], tombstone: bool) -> SyncRecord {
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

fn segment(
    signing_key: &SigningKey,
    sequence: u64,
    records: Vec<SyncRecord>,
    previous: Option<[u8; 32]>,
) -> SignedSegment {
    segment_at(signing_key, sequence, records, previous, 10, 11)
}

fn segment_at(
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
    drop(runtime);

    let restored = load(temporary.path());
    assert_eq!(
        restored.initial_peer_backfill_checkpoint("node-b"),
        Some(InitialPeerBackfillCheckpoint {
            page_cursor: Some("opaque-page-cursor".to_owned()),
            stream_state: streams,
            saw_history: true,
            completed: false,
            epoch: 0,
        })
    );
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
        .tiered_backfill_page(None, 1)
        .expect("first bounded tiered page");
    assert_eq!(first.records.len(), 1);
    let second = runtime
        .tiered_backfill_page(first.next_cursor.as_deref(), 1)
        .expect("second bounded tiered page");
    assert_eq!(second.records.len(), 1);
    assert_ne!(first.records[0].record_key, second.records[0].record_key);
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

fn load(path: &std::path::Path) -> RepositoryReplicaRuntime {
    RepositoryReplicaRuntime::load(HistoryStorage::open(path)).expect("runtime")
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

#[test]
fn repository_history_persists_beyond_legacy_snapshot_limit_without_a_gap() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let runtime = load(temporary.path());
    let rows = (0..16_385)
        .map(|sequence| {
            StoredRecord {
                observed_at_unix_seconds: sequence as u64,
                received_at_unix_seconds: sequence as u64,
                source_node_id: "node-a".to_owned(),
                source_epoch: 7,
                stream: "runtime".to_owned(),
                sequence: sequence as u64,
                subject_node_id: "subject-a".to_owned(),
                observer_node_id: "node-a".to_owned(),
                schema_id: "runtime.v1".to_owned(),
                schema_version: 1,
                record_key: vec![0],
                payload: vec![0],
                tombstone: false,
            }
            .sqlite_row()
            .expect("sqlite row")
        })
        .collect::<Vec<_>>();

    runtime
        .storage
        .upsert_repository_history_records(&rows)
        .expect("store external repository history");

    assert_eq!(
        runtime
            .storage
            .repository_history_record_count()
            .expect("count persisted repository history"),
        16_385
    );
    assert!(runtime.snapshot.records.is_empty());
    assert!(runtime.snapshot.gaps.is_empty());
}

#[test]
fn quota_guard_stops_history_writes_at_ten_gib_without_stopping_control_plane() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut runtime = load(temporary.path());
    runtime
        .force_capacity_for_test(
            DEFAULT_HISTORY_REPOSITORY_QUOTA_BYTES,
            HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES,
        )
        .expect("capacity");

    assert_eq!(
        runtime.history_write_availability(),
        HistoryWriteAvailability::QuotaReached
    );
    assert!(runtime.control_plane_permitted());
}

#[test]
fn persisted_fork_quarantine_and_stale_replica_rebuild_fail_closed() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let key = signing_key();
    let identity = identity(&key);
    let first = segment(&key, 0, vec![record(b"one", false)], None);
    let mut runtime = load(temporary.path());
    runtime
        .receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            11,
        )
        .expect("first segment");
    let fork = segment(&key, 0, vec![record(b"fork", false)], None);
    assert!(matches!(
        runtime.receive_wire(
            "cluster-a",
            &identity,
            &fork.wire_bytes().expect("wire"),
            12
        ),
        Err(RepositoryRuntimeError::Protocol(
            ProtocolError::ForkDetected { next_epoch: 8 }
        ))
    ));
    let mut restored = load(temporary.path());
    assert!(matches!(
        restored.receive_wire(
            "cluster-a",
            &identity,
            &first.wire_bytes().expect("wire"),
            13
        ),
        Err(RepositoryRuntimeError::Protocol(ProtocolError::Quarantined))
    ));
    let rebuild_at = 11 + TOMBSTONE_HORIZON_SECONDS + 1;
    let rebuilt = CanonicalSegment::new(
        "cluster-a",
        Cursor::new("node-a", 7, "runtime", 0).expect("cursor"),
        vec![record(b"rebuilt", false)],
        None,
        rebuild_at,
        rebuild_at,
    )
    .expect("rebuilt segment")
    .sign(&key)
    .expect("signature");
    restored
        .receive_wire(
            "cluster-a",
            &identity,
            &rebuilt.wire_bytes().expect("wire"),
            rebuild_at,
        )
        .expect("stale replica rebuilds before accepting replacement stream");
    let response = restored
        .query(
            "repository-a",
            HistoryQuery::new(rebuild_at, rebuild_at, 10).expect("query"),
            LocalQueryMetadata::current_window(rebuild_at),
        )
        .expect("query response");
    assert_eq!(response.records.len(), 1);
}
