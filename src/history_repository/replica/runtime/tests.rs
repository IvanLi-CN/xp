use ed25519_dalek::SigningKey;

use super::*;
use crate::{
    history_sync::{CanonicalSegment, SyncRecord},
    state::history_repository::{
        HistoryStorage,
        control::{HISTORY_REPOSITORY_LOW_SPACE_GUARD_BYTES, HistoryWriteAvailability},
        identity::{Ed25519PublicKey, RepositoryNodeId, X25519PublicKey},
        query::Completeness,
    },
};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[11; 32])
}

fn identity(key: &SigningKey) -> RepositoryNodeIdentity {
    RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
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
fn replica_repair_does_not_converge_when_gap_metadata_differs() {
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
    assert!(runtime.missing_segment_ids(&remote, true).is_err());
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
    for _ in 0..3 {
        runtime
            .record_collection_cycle("source-a", &ready, assignment.primary(), false)
            .expect("record primary failure");
    }
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
