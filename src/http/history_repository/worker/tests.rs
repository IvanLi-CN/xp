use super::*;
use crate::state::history_repository::{
    HistoryStorage,
    identity::{Ed25519PublicKey, RepositoryNodeId, RepositoryNodeIdentity, X25519PublicKey},
    replica::RepositoryReplicaRuntime,
};
use ed25519_dalek::SigningKey;

#[test]
fn dynamic_relay_keys_are_deterministic_per_cluster_and_recipient() {
    let sender = relay_keypair_from_cluster_material("cluster-a", "repository-a", "ca-key");
    let recipient = relay_keypair_from_cluster_material("cluster-a", "repository-b", "ca-key");
    let frame = RelayFrame::seal(
        sender,
        recipient.public_key(),
        [3; 12],
        b"repair batch",
        b"repository-b",
    )
    .expect("seal relay frame");
    assert_eq!(
        frame
            .open(
                relay_keypair_from_cluster_material("cluster-a", "repository-b", "ca-key"),
                b"repository-b",
            )
            .expect("open relay frame"),
        b"repair batch"
    );
    assert_ne!(
        recipient.public_key(),
        relay_keypair_from_cluster_material("cluster-a", "repository-c", "ca-key").public_key()
    );
}

#[test]
fn relay_repair_does_not_complete_daily_deep_verification() {
    assert_eq!(
        completed_replication_work(ReplicaWork::DeepVerification, false),
        ReplicaWork::AntiEntropy
    );
    assert_eq!(
        completed_replication_work(ReplicaWork::DeepVerification, true),
        ReplicaWork::DeepVerification
    );
}

#[test]
fn deep_partition_mismatch_restarts_tiered_backfill_after_segment_drain() {
    assert!(deep_repair_requires_tiered_backfill(
        ReplicaWork::DeepVerification,
        false,
        true,
    ));
    assert!(!deep_repair_requires_tiered_backfill(
        ReplicaWork::DeepVerification,
        true,
        true,
    ));
    assert!(!deep_repair_requires_tiered_backfill(
        ReplicaWork::DeepVerification,
        false,
        false,
    ));
    assert!(!deep_repair_requires_tiered_backfill(
        ReplicaWork::AntiEntropy,
        false,
        true,
    ));
}

#[test]
fn deep_repair_keeps_backfill_checkpoint_after_a_full_segment_batch() {
    let source_directory = tempfile::tempdir().expect("source directory");
    let target_directory = tempfile::tempdir().expect("target directory");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let identity = RepositoryNodeIdentity::new(
        RepositoryNodeId::try_from("node-a".to_owned()).expect("node id"),
        Ed25519PublicKey::from_bytes(signing_key.verifying_key().to_bytes()).expect("signing key"),
        X25519PublicKey::from_bytes([12; 32]).expect("relay key"),
    )
    .expect("identity");
    let mut source = RepositoryReplicaRuntime::load(HistoryStorage::open(source_directory.path()))
        .expect("source runtime");
    let mut target = RepositoryReplicaRuntime::load(HistoryStorage::open(target_directory.path()))
        .expect("target runtime");
    let mut previous = None;
    for sequence in 0..65_u64 {
        let segment = CanonicalSegment::new(
            "cluster-a",
            Cursor::new("node-a", 7, "runtime", sequence).expect("cursor"),
            vec![SyncRecord::new(
                "subject-a",
                "node-a",
                "runtime.v1",
                1,
                format!("record-{sequence}").into_bytes(),
                b"sample".to_vec(),
                false,
            )],
            previous,
            10 + sequence,
            11 + sequence,
        )
        .expect("segment")
        .sign(&signing_key)
        .expect("signature");
        previous = Some(segment.segment_hash().expect("segment hash"));
        source
            .receive_wire(
                "cluster-a",
                &identity,
                &segment.wire_bytes().expect("wire"),
                100 + sequence,
            )
            .expect("store source segment");
    }

    let summary = source.replication_summary().expect("source summary");
    assert_eq!(summary.segment_ids.len(), 65);
    let first_batch = target
        .missing_segment_ids(&summary, true)
        .expect("first bounded batch");
    assert_eq!(first_batch.len(), 64);
    for segment in source
        .repair_batch(&first_batch)
        .expect("first repair batch")
        .segments
    {
        target
            .receive_wire("cluster-a", &identity, &segment.wire, 200)
            .expect("receive bounded repair segment");
    }

    assert!(
        target
            .requires_repair(&summary, true)
            .expect("remaining segment keeps summary incomplete")
    );
    assert!(
        !target
            .missing_segment_ids(&summary, true)
            .expect("remaining segment repair")
            .is_empty()
    );
    let stream_state = BTreeMap::from([("tiered-history".to_owned(), (64, Some([7; 32])))]);
    target
        .update_initial_peer_backfill_checkpoint(
            "node-b",
            Some("tiered-page-cursor".to_owned()),
            stream_state,
            true,
            false,
        )
        .expect("seed tiered checkpoint");
    let checkpoint = target
        .initial_peer_backfill_checkpoint("node-b")
        .expect("tiered checkpoint");

    assert!(
        !restart_tiered_backfill_after_incomplete_deep_repair(
            &mut target,
            "node-b",
            ReplicaWork::DeepVerification,
            true,
            true,
        )
        .expect("retain tiered checkpoint")
    );
    assert_eq!(
        target.initial_peer_backfill_checkpoint("node-b"),
        Some(checkpoint)
    );

    let second_batch = target
        .missing_segment_ids(&summary, true)
        .expect("second bounded batch");
    assert_eq!(second_batch.len(), 1);
    for segment in source
        .repair_batch(&second_batch)
        .expect("second repair batch")
        .segments
    {
        target
            .receive_wire("cluster-a", &identity, &segment.wire, 201)
            .expect("receive final repair segment");
    }
    assert!(
        target
            .missing_segment_ids(&summary, true)
            .expect("all summary segments repaired")
            .is_empty()
    );
    assert!(
        !target
            .requires_repair(&summary, true)
            .expect("full summary is converged")
    );
}

#[test]
fn truncated_repair_response_leaves_only_undelivered_segments_pending() {
    let first = vec![1_u8, 2, 3];
    let second = vec![4_u8, 5, 6];
    let third = vec![7_u8, 8, 9];
    let first_id = hex::encode(Sha256::digest(&first));
    let second_id = hex::encode(Sha256::digest(&second));
    let third_id = hex::encode(Sha256::digest(&third));
    let mut pending = [first_id, second_id, third_id.clone()]
        .into_iter()
        .collect();

    remove_delivered_repair_segment_ids(&mut pending, [first.as_slice(), second.as_slice()])
        .expect("partial repair response advances pending ids");

    assert_eq!(pending.into_iter().collect::<Vec<_>>(), vec![third_id]);
}

#[test]
fn source_deletion_producer_queues_the_independent_tombstone_before_matching_history() {
    let historical_key = b"node-history:node:node-a:daily-traffic:2026-08-14".to_vec();
    let records = source_records_with_deletions(
        "node-a",
        100,
        vec![(
            "traffic.v1".to_owned(),
            b"node-history:node:node-a:".to_vec(),
            Some("node-a".to_owned()),
        )],
        vec![
            source_record_with_key(
                "traffic.v1",
                "node-a",
                100,
                historical_key.clone(),
                serde_json::json!({ "sample": true }),
                false,
            )
            .expect("historical source record"),
        ],
    )
    .expect("production source records");

    assert!(records[0].is_tombstone());
    assert_eq!(records[0].schema().0, "traffic.v1");
    assert_eq!(records[0].record_key(), b"node-history:node:node-a:");
    assert!(!records[1].is_tombstone());
    assert_eq!(records[1].record_key(), historical_key);
}

#[test]
fn completed_catch_up_starts_a_stability_window_without_rechecking_live_segments() {
    assert!(!super::should_run_initial_catch_up(true));
    assert!(super::should_run_initial_catch_up(false));
}

#[test]
fn syncing_tombstone_backfill_checkpoint() {
    assert!(!super::should_fanout_tombstone_acknowledgements(
        RepositoryLifecycle::Syncing
    ));
    assert!(super::should_fanout_tombstone_acknowledgements(
        RepositoryLifecycle::Ready
    ));
}

#[test]
fn expired_peer_export_cursor_restarts_only_after_an_application_rejection() {
    let application = RepositoryDirectError::Application(anyhow::anyhow!("409 conflict"));
    assert!(super::should_restart_peer_backfill(
        Some("expired-cursor"),
        &application
    ));
    assert!(!super::should_restart_peer_backfill(None, &application));
    let transport = RepositoryDirectError::Transport(anyhow::anyhow!("connection reset"));
    assert!(!super::should_restart_peer_backfill(
        Some("expired-cursor"),
        &transport
    ));
}

#[test]
fn peer_backfill_streams_are_independent_from_the_live_source_cursor_chain() {
    assert_eq!(
        super::peer_backfill_stream_for_schema("traffic.v1", "node-a").expect("backfill stream"),
        "traffic-backfill-node-a"
    );
    assert_ne!(
        super::peer_backfill_stream_for_schema("traffic.v1", "node-a").expect("backfill stream"),
        super::source_stream_for_schema("traffic.v1").expect("live stream")
    );
    assert_eq!(
        super::source_stream_for_schema(crate::resource_monitoring::RESOURCE_HISTORY_SCHEMA),
        Some(crate::resource_monitoring::RESOURCE_HISTORY_STREAM)
    );
}

#[test]
fn peer_backfill_tombstones_use_the_independent_tombstone_stream() {
    let tombstone = source_record_with_key(
        "traffic.v1",
        "node-a",
        100,
        b"node-history:node:node-a:".to_vec(),
        serde_json::json!({ "deleted": true }),
        true,
    )
    .expect("tombstone");
    assert_eq!(
        super::peer_backfill_stream_for_record(&tombstone, "node-a").expect("backfill stream"),
        "tombstone"
    );
}

#[test]
fn initial_backfill_progress() {
    let mut collector = super::HistoricalBackfillCollector::new(None, 128);
    for sequence in 0..130_u64 {
        collector
            .push((
                sequence,
                source_record_with_key(
                    "runtime.v1",
                    "node-a",
                    sequence,
                    format!("node-history:node:node-a:{sequence}").into_bytes(),
                    serde_json::json!({ "sequence": sequence }),
                    false,
                )
                .expect("bounded record"),
            ))
            .expect("bounded page");
    }
    assert_eq!(collector.records.len(), 128);
    assert!(collector.has_more);
    assert_eq!(
        collector
            .records
            .first_key_value()
            .expect("first record")
            .1
            .0,
        0
    );
    assert_eq!(
        collector.records.last_key_value().expect("last record").1.0,
        127
    );
    let cursor = collector
        .next_cursor()
        .expect("cursor encoding")
        .expect("more history");
    let mut next = super::HistoricalBackfillCollector::new(
        Some(super::HistoricalBackfillSortKey::decode(&cursor).expect("cursor decoding")),
        128,
    );
    for sequence in 0..130_u64 {
        next.push((
            sequence,
            source_record_with_key(
                "runtime.v1",
                "node-a",
                sequence,
                format!("node-history:node:node-a:{sequence}").into_bytes(),
                serde_json::json!({ "sequence": sequence }),
                false,
            )
            .expect("bounded record"),
        ))
        .expect("bounded page");
    }
    assert_eq!(next.records.len(), 2);
    assert!(!next.has_more);
    assert_eq!(
        next.records.first_key_value().expect("first record").1.0,
        128
    );
    assert_eq!(next.records.last_key_value().expect("last record").1.0, 129);
}

#[test]
fn initial_backfill_rejects_a_record_over_the_byte_budget() {
    let mut collector = super::HistoricalBackfillCollector::new(None, 128);
    let result = collector.push((
        0,
        SyncRecord::new(
            "node-a",
            "node-a",
            "runtime.v1",
            1,
            b"oversized".to_vec(),
            vec![b'x'; 192 * 1024],
            false,
        ),
    ));
    assert!(result.is_err());
}

#[test]
fn initial_backfill_splits_same_timestamp_records_before_canonical_limit() {
    let records = (0..8_u64)
        .map(|sequence| {
            (
                100,
                SyncRecord::new(
                    "node-a",
                    "node-a",
                    "runtime.v1",
                    1,
                    sequence.to_be_bytes().to_vec(),
                    vec![b'x'; 32 * 1024],
                    false,
                ),
            )
        })
        .collect();
    let batches = super::backfill::historical_record_batches(records).expect("split batches");
    assert!(batches.len() > 1);
    for (_, records) in batches {
        let segment = CanonicalSegment::new(
            "backfill",
            Cursor::new("backfill", 0, "runtime", 0).expect("cursor"),
            records,
            None,
            100,
            100,
        )
        .expect("bounded segment");
        assert!(segment.canonical_bytes().expect("canonical bytes").len() <= 192 * 1024);
    }
}

#[test]
fn initial_backfill_queues_each_batch_with_its_historical_observed_time() {
    let records = [100_u64, 200]
        .into_iter()
        .map(|observed_at| {
            (
                observed_at,
                source_record_with_key(
                    "runtime.v1",
                    "node-a",
                    observed_at,
                    observed_at.to_be_bytes().to_vec(),
                    serde_json::json!({ "observed_at": observed_at }),
                    false,
                )
                .expect("historical record"),
            )
        })
        .collect();
    let batches = super::backfill::historical_record_batches(records).expect("split batches");

    let queued = super::backfill::queued_history_backfill_batches(batches);

    assert_eq!(
        queued
            .iter()
            .map(|(_, observed_at)| *observed_at)
            .collect::<Vec<_>>(),
        vec![100, 200]
    );
}

#[test]
fn ordinary_backfill_places_tombstones_in_the_first_collector_phase() {
    let mut collector = super::HistoricalBackfillCollector::new(None, 2);
    collector
        .push_with_times(
            0,
            1_700_000_000,
            super::source_record_with_key(
                "traffic.v1",
                "node-a",
                0,
                b"node-history:node:node-a:".to_vec(),
                serde_json::json!({ "deleted": true }),
                true,
            )
            .expect("tombstone"),
        )
        .expect("tombstone");
    collector
        .push((
            100,
            super::source_record_with_key(
                "traffic.v1",
                "node-a",
                100,
                b"node-history:node:node-a:old".to_vec(),
                serde_json::json!({ "old": true }),
                false,
            )
            .expect("history"),
        ))
        .expect("history");
    assert!(
        collector
            .records
            .first_key_value()
            .expect("first record")
            .1
            .1
            .is_tombstone()
    );
    assert_eq!(
        collector
            .records
            .first_key_value()
            .expect("first record")
            .1
            .0,
        1_700_000_000
    );
}
