use super::*;

#[derive(Debug, Default)]
struct TestGeoLookup;

impl GeoLookup for TestGeoLookup {
    fn lookup(&self, _ip: &str) -> PersistedInboundIpGeo {
        PersistedInboundIpGeo::default()
    }
}

fn sample(
    membership_key: &str,
    user_id: &str,
    node_id: &str,
    endpoint_id: &str,
    endpoint_tag: &str,
    ips: &[&str],
) -> InboundIpMinuteSample {
    InboundIpMinuteSample {
        membership_key: membership_key.to_string(),
        user_id: user_id.to_string(),
        node_id: node_id.to_string(),
        endpoint_id: endpoint_id.to_string(),
        endpoint_tag: endpoint_tag.to_string(),
        ips: ips.iter().map(|ip| (*ip).to_string()).collect(),
    }
}

#[derive(Debug, Default)]
struct FixedGeoLookup;

impl GeoLookup for FixedGeoLookup {
    fn lookup(&self, ip: &str) -> PersistedInboundIpGeo {
        if ip == "8.8.8.8" {
            PersistedInboundIpGeo {
                country: "US".to_string(),
                region: "CA".to_string(),
                city: "Mountain View".to_string(),
                operator: "Google LLC".to_string(),
            }
        } else {
            PersistedInboundIpGeo::default()
        }
    }
}

#[test]
fn format_region_prefers_city_when_region_is_only_a_subdivision_code() {
    let geo = PersistedInboundIpGeo {
        country: "DE".to_string(),
        region: "HH".to_string(),
        city: "Hamburg".to_string(),
        operator: String::new(),
    };
    assert_eq!(format_region(&geo), "DE Hamburg (HH)");

    let geo = PersistedInboundIpGeo {
        country: "US".to_string(),
        region: "California".to_string(),
        city: "Mountain View".to_string(),
        operator: String::new(),
    };
    assert_eq!(format_region(&geo), "US California");
}

#[test]
fn geo_lookup_reuses_persisted_geo_across_memberships() {
    let mut usage = PersistedInboundIpUsage::default();
    let seed = FixedGeoLookup;
    let noop = TestGeoLookup;

    let minute0 = chrono::DateTime::parse_from_rfc3339("2026-03-08T10:11:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let minute1 = minute0 + Duration::minutes(1);

    usage.record_minute_samples(
        minute0,
        false,
        &[sample("u1::e1", "u1", "n1", "e1", "ep-1", &["8.8.8.8"])],
        &seed,
        true,
    );
    let cached = usage.memberships["u1::e1"].ips["8.8.8.8"].geo.clone();
    assert!(!geo_is_default(&cached));

    // The same IP seen under a new membership should not trigger another lookup, and should
    // reuse the persisted geo from the existing record.
    assert!(
        usage
            .collect_lookup_candidates(
                minute1,
                &[sample("u2::e2", "u2", "n1", "e2", "ep-2", &["8.8.8.8"],)]
            )
            .is_empty()
    );

    usage.record_minute_samples(
        minute1,
        false,
        &[sample("u2::e2", "u2", "n1", "e2", "ep-2", &["8.8.8.8"])],
        &noop,
        true,
    );
    assert_eq!(usage.memberships["u2::e2"].ips["8.8.8.8"].geo, cached);
}

#[test]
fn geo_lookup_can_be_disabled_to_avoid_reusing_persisted_geo() {
    let mut usage = PersistedInboundIpUsage::default();
    let seed = FixedGeoLookup;
    let noop = TestGeoLookup;

    let minute0 = chrono::DateTime::parse_from_rfc3339("2026-03-08T10:11:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let minute1 = minute0 + Duration::minutes(1);

    usage.record_minute_samples(
        minute0,
        false,
        &[sample("u1::e1", "u1", "n1", "e1", "ep-1", &["8.8.8.8"])],
        &seed,
        true,
    );
    assert!(!geo_is_default(
        &usage.memberships["u1::e1"].ips["8.8.8.8"].geo
    ));

    // When geo is disabled, new memberships should not copy persisted geo.
    usage.record_minute_samples(
        minute1,
        false,
        &[sample("u2::e2", "u2", "n1", "e2", "ep-2", &["8.8.8.8"])],
        &noop,
        false,
    );
    assert!(geo_is_default(
        &usage.memberships["u2::e2"].ips["8.8.8.8"].geo
    ));
}

#[test]
fn collect_lookup_candidates_includes_geo_after_full_window_shift() {
    let mut usage = PersistedInboundIpUsage::default();
    let seed = FixedGeoLookup;

    let minute0 = chrono::DateTime::parse_from_rfc3339("2026-03-08T10:11:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let minute_far = minute0 + Duration::minutes(MINUTES_WINDOW as i64);

    usage.record_minute_samples(
        minute0,
        false,
        &[sample("u1::e1", "u1", "n1", "e1", "ep-1", &["8.8.8.8"])],
        &seed,
        true,
    );
    assert!(!geo_is_default(
        &usage.memberships["u1::e1"].ips["8.8.8.8"].geo
    ));

    // After shifting a full window, the old bitmap drops out and the record will be pruned
    // during `advance_to_minute()`. Candidate selection must treat the geo as missing and
    // request a fresh prime for the new minute.
    let candidates = usage.collect_lookup_candidates(
        minute_far,
        &[sample("u1::e1", "u1", "n1", "e1", "ep-1", &["8.8.8.8"])],
    );
    assert_eq!(candidates, vec!["8.8.8.8".to_string()]);
}

#[test]
fn collect_lookup_candidates_filters_non_global_ips() {
    let usage = PersistedInboundIpUsage::default();
    let minute0 = chrono::DateTime::parse_from_rfc3339("2026-03-08T10:11:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let candidates = usage.collect_lookup_candidates(
        minute0,
        &[sample(
            "u1::e1",
            "u1",
            "n1",
            "e1",
            "ep-1",
            &["10.0.0.1", "203.0.113.7", "8.8.8.8"],
        )],
    );
    assert_eq!(candidates, vec!["8.8.8.8".to_string()]);
}

#[test]
fn record_and_shift_bitmap_window() {
    let mut usage = PersistedInboundIpUsage::default();
    let resolver = TestGeoLookup;
    let minute0 = chrono::DateTime::parse_from_rfc3339("2026-03-08T10:11:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let minute1 = minute0 + Duration::minutes(1);

    assert!(usage.record_minute_samples(
        minute0,
        false,
        &[sample("u1::e1", "u1", "n1", "e1", "ep-1", &["203.0.113.7"])],
        &resolver,
        true,
    ));
    assert_eq!(usage.memberships["u1::e1"].ips["203.0.113.7"].minutes, 1);

    assert!(usage.record_minute_samples(
        minute1,
        false,
        &[sample(
            "u1::e1",
            "u1",
            "n1",
            "e1",
            "ep-1",
            &["203.0.113.7", "203.0.113.8"],
        )],
        &resolver,
        true,
    ));
    let record = &usage.memberships["u1::e1"].ips["203.0.113.7"];
    assert_eq!(record.minutes, 2);
    assert!(get_bit(&record.bitmap, MINUTES_WINDOW - 1));
    assert!(get_bit(&record.bitmap, MINUTES_WINDOW - 2));
    assert_eq!(usage.memberships["u1::e1"].ips["203.0.113.8"].minutes, 1);
}

#[test]
fn normalize_recomputes_minutes_and_prunes_memberships() {
    let mut usage = PersistedInboundIpUsage {
        latest_minute: Some("2026-03-08T10:11:00Z".to_string()),
        memberships: BTreeMap::from([(
            "u1::e1".to_string(),
            PersistedInboundIpMembership {
                user_id: "u1".to_string(),
                node_id: "n1".to_string(),
                endpoint_id: "e1".to_string(),
                endpoint_tag: "ep-1".to_string(),
                ips: BTreeMap::from([(
                    "203.0.113.7".to_string(),
                    PersistedInboundIpRecord {
                        bitmap: {
                            let mut bitmap = zero_bitmap();
                            set_bit(&mut bitmap, MINUTES_WINDOW - 1, true);
                            bitmap
                        },
                        minutes: 99,
                        first_seen_at: "2026-03-08T10:11:00Z".to_string(),
                        last_seen_at: "2026-03-08T10:11:00Z".to_string(),
                        geo: PersistedInboundIpGeo::default(),
                    },
                )]),
            },
        )]),
        ..PersistedInboundIpUsage::default()
    };

    let changed = usage.normalize(&BTreeSet::from(["u1::e1".to_string()]));
    assert!(changed);
    assert_eq!(usage.memberships["u1::e1"].ips["203.0.113.7"].minutes, 1);

    let changed = usage.normalize(&BTreeSet::new());
    assert!(changed);
    assert!(usage.memberships.is_empty());
}

#[test]
fn build_window_view_deduplicates_unique_ip_counts_and_merges_segments() {
    let mut usage = PersistedInboundIpUsage::default();
    let resolver = TestGeoLookup;
    let minute0 = chrono::DateTime::parse_from_rfc3339("2026-03-08T10:11:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let minute1 = minute0 + Duration::minutes(1);
    let minute2 = minute1 + Duration::minutes(1);

    usage.record_minute_samples(
        minute0,
        false,
        &[
            sample("u1::e1", "u1", "n1", "e1", "ep-1", &["203.0.113.7"]),
            sample("u2::e2", "u2", "n1", "e2", "ep-2", &["203.0.113.7"]),
        ],
        &resolver,
        true,
    );
    usage.record_minute_samples(
        minute1,
        false,
        &[
            sample("u1::e1", "u1", "n1", "e1", "ep-1", &["203.0.113.7"]),
            sample("u2::e2", "u2", "n1", "e2", "ep-2", &["198.51.100.9"]),
        ],
        &resolver,
        true,
    );
    usage.record_minute_samples(
        minute2,
        false,
        &[sample("u1::e1", "u1", "n1", "e1", "ep-1", &["203.0.113.7"])],
        &resolver,
        true,
    );

    let view = build_window_view(
        &usage,
        minute2,
        InboundIpUsageWindow::Hours24,
        &[
            InboundIpUsageMembershipView {
                membership_key: "u1::e1".to_string(),
                endpoint_id: "e1".to_string(),
                endpoint_tag: "ep-1".to_string(),
            },
            InboundIpUsageMembershipView {
                membership_key: "u2::e2".to_string(),
                endpoint_id: "e2".to_string(),
                endpoint_tag: "ep-2".to_string(),
            },
        ],
        Vec::new(),
    );

    let tail = &view.unique_ip_series[view.unique_ip_series.len() - 3..];
    assert_eq!(tail[0].count, 1);
    assert_eq!(tail[1].count, 2);
    assert_eq!(tail[2].count, 1);

    let ip = view
        .ips
        .iter()
        .find(|item| item.ip == "203.0.113.7")
        .unwrap();
    assert_eq!(ip.minutes, 3);
    assert_eq!(
        ip.endpoint_tags,
        vec!["ep-1".to_string(), "ep-2".to_string()]
    );

    let lane = view
        .timeline
        .iter()
        .find(|lane| lane.lane_key == "ep-1|203.0.113.7")
        .unwrap();
    assert_eq!(lane.minutes, 3);
    assert_eq!(lane.segments.len(), 1);
    assert_eq!(lane.segments[0].start_minute, "2026-03-08T10:11:00+00:00");
    assert_eq!(lane.segments[0].end_minute, "2026-03-08T10:13:00+00:00");
}
