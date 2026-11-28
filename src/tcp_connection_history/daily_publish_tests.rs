use super::test_support::parse_minute;
use super::*;
fn store_with_day(dir: &Path, day: &str) -> TcpConnectionHistoryStore {
    let mut store = TcpConnectionHistoryStore::open(dir).unwrap();
    let node = xp_test_fixtures::label_node1_variant2();
    store
        .record_minute(
            node,
            parse_minute(day),
            &FullSiteMinuteSample::new(
                BTreeMap::from([(
                    (
                        ConnectionClass::Inbound,
                        xp_test_fixtures::endpoint_fixtureinline_a().to_string(),
                    ),
                    3_u32,
                )]),
                false,
                false,
                vec![ConnectionClass::Inbound],
            ),
        )
        .unwrap();
    store
        .refresh_rollups_for_minute(node, parse_minute(day))
        .unwrap();
    store
}

#[test]
fn an_incomplete_day_is_not_published() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_day(dir.path(), "2026-11-26T10:00:00Z");
    let pending = store
        .pending_daily_buckets(
            xp_test_fixtures::label_node1_variant2(),
            16,
            parse_minute("2026-11-26T10:30:00Z"),
        )
        .unwrap();
    assert!(
        pending.is_empty(),
        "the current UTC day must stay unpublished"
    );
}

#[test]
fn a_completed_day_is_published_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_day(dir.path(), "2026-11-26T10:00:00Z");
    let node = xp_test_fixtures::label_node1_variant2();
    let pending = store
        .pending_daily_buckets(node, 16, parse_minute("2026-11-27T00:05:00Z"))
        .unwrap();
    assert_eq!(pending.len(), 1);

    store.mark_daily_buckets_delivered(node, &pending).unwrap();
    let again = store
        .pending_daily_buckets(node, 16, parse_minute("2026-11-27T00:10:00Z"))
        .unwrap();
    assert!(again.is_empty(), "a delivered day must not be republished");
}

#[test]
fn rebuilding_a_bucket_keeps_its_delivered_flag() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_day(dir.path(), "2026-11-26T10:00:00Z");
    let node = xp_test_fixtures::label_node1_variant2();
    let daily_bucket =
        floor_bucket(parse_minute("2026-11-26T10:00:00Z"), Resolution::Daily).timestamp();
    store
        .mark_daily_buckets_delivered(node, &[daily_bucket])
        .unwrap();

    store
        .refresh_rollups_for_minute(node, parse_minute("2026-11-26T10:01:00Z"))
        .unwrap();
    let pending = store
        .pending_daily_buckets(node, 16, parse_minute("2026-11-27T00:05:00Z"))
        .unwrap();
    assert!(
        pending.is_empty(),
        "a rollup refresh must not re-queue a delivered day"
    );
}
