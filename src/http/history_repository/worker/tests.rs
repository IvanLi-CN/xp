use super::*;

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
fn catch_up_rechecks_a_peer_after_repair_before_declaring_it_complete() {
    assert!(super::backfill::catch_up_has_verification_retry(0));
    assert!(!super::backfill::catch_up_has_verification_retry(1));
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
