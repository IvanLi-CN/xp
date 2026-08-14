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
fn peer_transport_failure_keeps_the_first_repository_syncing() {
    assert!(!super::initial_backfill_is_complete(
        true,
        &[Some(true), None]
    ));
    assert!(super::initial_backfill_is_complete(false, &[Some(false)]));
    assert!(super::initial_backfill_is_complete(
        true,
        &[Some(false), Some(true)]
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
fn initial_history_backfill_collector_keeps_only_one_bounded_page() {
    let mut collector = super::HistoricalBackfillCollector::new(None, 64);
    for sequence in 0..130_u64 {
        collector.push((
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
        ));
    }
    assert_eq!(collector.records.len(), 64);
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
        63
    );
    let cursor = collector
        .next_cursor()
        .expect("cursor encoding")
        .expect("more history");
    let mut next = super::HistoricalBackfillCollector::new(
        Some(super::HistoricalBackfillSortKey::decode(&cursor).expect("cursor decoding")),
        64,
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
        ));
    }
    assert_eq!(next.records.len(), 64);
    assert!(next.has_more);
    assert_eq!(
        next.records.first_key_value().expect("first record").1.0,
        64
    );
    assert_eq!(next.records.last_key_value().expect("last record").1.0, 127);
}
